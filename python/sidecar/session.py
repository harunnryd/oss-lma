import asyncio
import logging
import sqlite3
import time

from lma_pipeline import SegmentAssembler
from lma_stt.types import MeetingContext, ProviderAuthError, ProviderResetError
from websockets.exceptions import ConnectionClosed

from sidecar.frames import (
    INVALID_FRAME_CLOSE_CODE,
    INVALID_FRAME_CODE,
    INVALID_FRAME_CONTEXT,
    AgentQuery,
    End,
    FrameError,
    Pause,
    Resume,
    SpeakerChange,
    Start,
    VpCommand,
    error_frame,
    parse_frame,
    serialize_event,
)
from sidecar.reconnect import ReconnectState
from sidecar.storage.persistence import PersistenceWriter
from sidecar.storage.recording import RecordingSink, WavRecordingSink

DRAIN_TIMEOUT_SECONDS = 10.0
THINKING_STEP_STUB_CONTENT = "agent unavailable in P1"

logger = logging.getLogger(__name__)


class Session:
    def __init__(
        self,
        connection,
        engine_factory,
        *,
        db: PersistenceWriter | None = None,
        recorder: RecordingSink | None = None,
        record_meeting: bool = False,
    ) -> None:
        self.connection = connection
        self.engine_factory = engine_factory
        self.db = db
        self.recorder = recorder
        self.record_meeting = record_meeting
        self.call_id = ""
        self.stream = None
        self.assembler = None
        self.pump_task: asyncio.Task | None = None
        self.paused = False
        self.chunk_bytes = 0
        self.send_lock = asyncio.Lock()
        self.time_offset_ms: int = 0
        self.reconnect_state = ReconnectState()
        self._stream_started_at_ms: int | None = None

    def _apply_offset(self, event: dict, offset_ms: int) -> dict:
        if offset_ms == 0 or event.get("EventType") != "ADD_TRANSCRIPT_SEGMENT":
            return event
        return {
            **event,
            "StartTime": round(event["StartTime"] * 1000 + offset_ms) / 1000,
            "EndTime": round(event["EndTime"] * 1000 + offset_ms) / 1000,
        }

    async def run(self) -> None:
        try:
            async for message in self.connection:
                if isinstance(message, bytes):
                    await self.on_binary(message)
                else:
                    await self.on_text(message)
        finally:
            await self.shutdown()

    async def on_text(self, raw: str) -> None:
        try:
            frame = parse_frame(raw)
        except FrameError:
            await self._reject_invalid_frame()
            return
        match frame:
            case Start():
                await self._start_session(frame)
            case SpeakerChange():
                if self.assembler is not None:
                    self.assembler.set_active_speaker(frame.channel, frame.active_speaker)
            case Pause():
                if self.stream is not None:
                    self.paused = True
            case Resume():
                if self.stream is not None:
                    self.paused = False
            case End():
                await self._close_session(drain=True)
            case AgentQuery():
                await self._send({
                    "EventType": "THINKING_STEP",
                    "CallId": frame.call_id,
                    "QueryId": frame.query_id,
                    "Seq": 0,
                    "StepType": "status",
                    "Content": THINKING_STEP_STUB_CONTENT,
                })
            case VpCommand():
                pass
            case _:
                pass

    async def on_binary(self, pcm: bytes) -> None:
        if self.stream is None:
            await self.connection.close(1008, "audio-before-start")
            return
        if len(pcm) != self.chunk_bytes:
            await self._reject_invalid_frame()
            return
        if self.paused:
            return
        try:
            await self.stream.feed(pcm)
            if self.recorder is not None:
                self.recorder.feed(pcm)
        except ValueError:
            await self._reject_invalid_frame()

    async def shutdown(self) -> None:
        await self._close_session(drain=False)

    async def _start_session(self, frame: Start) -> None:
        await self._close_session(drain=True)
        ctx: MeetingContext = {
            "call_id": frame.call_id,
            "sample_rate": frame.sampling_rate,
            "diarize": {
                "system": frame.diarize_system_channel,
                "mic": frame.diarize_mic_channel,
            },
            "language_hints": [],
        }
        self.call_id = frame.call_id
        self.chunk_bytes = frame.sampling_rate * 4 // 10
        self.paused = False
        engine = self.engine_factory(ctx)
        self.stream = await engine.start(ctx)
        self._stream_started_at_ms = int(time.time() * 1000)
        self.assembler = SegmentAssembler(frame.call_id)
        self.pump_task = asyncio.create_task(self._pump(self.stream, self.assembler))
        if self.record_meeting and self.recorder is None:
            import os
            from pathlib import Path

            base = Path(os.environ.get("LMA_RECORDING_DIR", str(Path.home() / "Library" / "Application Support" / "oss-lma" / "recordings")))
            wav_path = base / frame.call_id / "audio.wav"
            wav_path.parent.mkdir(parents=True, exist_ok=True)
            self.recorder = WavRecordingSink(wav_path)
        if self.db is not None:
            ev = {"EventType": "START", "CallId": frame.call_id}
            self.db.write(ev)
            self.time_offset_ms = self.db.write_meeting_started(ev, return_offset=True) or 0

    async def _close_session(self, drain: bool) -> None:
        if self.stream is None:
            return
        stream = self.stream
        pump_task = self.pump_task
        self.stream = None
        self.assembler = None
        self.pump_task = None
        self.paused = False
        if drain and pump_task is not None:
            try:
                await stream.close()
                await asyncio.wait_for(pump_task, DRAIN_TIMEOUT_SECONDS)
            except (ProviderResetError, ProviderAuthError, ConnectionClosed, OSError, TimeoutError):
                pass
            if self.db is not None:
                self.db.write({"EventType": "END", "CallId": self.call_id})
            if self.recorder is not None:
                self.recorder.stop()
            return
        if pump_task is not None:
            pump_task.cancel()
            await asyncio.gather(pump_task, return_exceptions=True)
        try:
            await stream.close()
        except (ProviderResetError, ProviderAuthError, ConnectionClosed, OSError):
            pass
        if self.recorder is not None:
            self.recorder.stop()

    async def _pump(self, stream, assembler) -> None:
        try:
            async for result in stream:
                for event in assembler.on_result(result):
                    adjusted = self._apply_offset(event, self.time_offset_ms)
                    if self.db is not None:
                        self.db.write(event, time_offset_ms=self.time_offset_ms)
                    await self._send(adjusted)
        except sqlite3.DatabaseError as exc:
            logger.exception("sqlite error in pump for call %s", self.call_id)
            await self._send(
                error_frame(self.call_id, "DB_WRITE_CONFLICT", {"reason": str(exc)})
            )
            return
        except ConnectionClosed:
            return
        except ProviderAuthError:
            logger.exception("stt provider auth error in pump for call %s", self.call_id)
            await self._send(error_frame(self.call_id, "STT_PROVIDER_AUTH"))
            return
        except ProviderResetError:
            logger.exception("stt provider reset error in pump for call %s", self.call_id)
            await self._send(error_frame(self.call_id, "STT_STREAM_RESET"))
            return
        except Exception:
            logger.exception("unexpected error in pump for call %s", self.call_id)
            await self._send(error_frame(self.call_id, "STT_STREAM_RESET"))
            return

    async def _reject_invalid_frame(self) -> None:
        await self._send(error_frame(self.call_id, INVALID_FRAME_CODE, INVALID_FRAME_CONTEXT))
        await self.connection.close(INVALID_FRAME_CLOSE_CODE, "invalid-frame")

    async def _send(self, event: dict) -> None:
        async with self.send_lock:
            await self.connection.send(serialize_event(event))
