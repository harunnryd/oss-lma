import asyncio
import logging
import sqlite3
import time
from collections.abc import Awaitable, Callable

from lma_pipeline import SegmentAssembler
from lma_stt.engine import SpeechEngine
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
from sidecar.reconnect import ReconnectState, load_reconnect_policy
from sidecar.storage.persistence import PersistenceWriter
from sidecar.storage.recording import RecordingSink, WavRecordingSink

DRAIN_TIMEOUT_SECONDS = 10.0
THINKING_STEP_STUB_CONTENT = "agent unavailable in P1"

logger = logging.getLogger(__name__)


class Session:
    def __init__(
        self,
        connection,
        engine_factory: Callable[[MeetingContext], SpeechEngine],
        *,
        db: PersistenceWriter | None = None,
        recorder: RecordingSink | None = None,
        record_meeting: bool = False,
        sleep: Callable[[float], Awaitable[None]] = asyncio.sleep,
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
        self._reconnecting = False
        self._sleep = sleep
        self._last_ctx: MeetingContext | None = None

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
                await self._send(
                    {
                        "EventType": "THINKING_STEP",
                        "CallId": frame.call_id,
                        "QueryId": frame.query_id,
                        "Seq": 0,
                        "StepType": "status",
                        "Content": THINKING_STEP_STUB_CONTENT,
                    }
                )
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
        if self._reconnecting:
            if self.recorder is not None:
                self.recorder.feed(pcm)
            return
        try:
            await self.stream.feed(pcm)
            if self.recorder is not None:
                self.recorder.feed(pcm)
        except ValueError:
            await self._reject_invalid_frame()
        except (ProviderResetError, ProviderAuthError, ConnectionClosed, OSError):
            logger.debug("dropped audio chunk for call %s: stt stream unavailable", self.call_id)

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
        self._reconnecting = False
        self._last_ctx = ctx
        engine = self.engine_factory(ctx)
        self.stream = await engine.start(ctx)
        self._stream_started_at_ms = self._now_ms()
        self.assembler = SegmentAssembler(frame.call_id)
        self.pump_task = asyncio.create_task(self._pump(self.stream, self.assembler))
        if self.record_meeting and self.recorder is None:
            import os
            from pathlib import Path

            base = Path(
                os.environ.get(
                    "LMA_RECORDING_DIR",
                    str(Path.home() / "Library" / "Application Support" / "oss-lma" / "recordings"),
                )
            )
            wav_path = base / frame.call_id / "audio.wav"
            wav_path.parent.mkdir(parents=True, exist_ok=True)
            self.recorder = WavRecordingSink(wav_path)
        if self.db is not None:
            ev = {"EventType": "START", "CallId": frame.call_id}
            stored_offset = self.db.write_meeting_started(ev, return_offset=True) or 0
            resumed_offset = self.db.read_max_segment_end_ms(frame.call_id) or 0
            self.time_offset_ms = max(stored_offset, resumed_offset)

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

    def _now_ms(self) -> int:
        return int(time.time() * 1000)

    def _ctx(self) -> MeetingContext | None:
        return self._last_ctx

    async def _pump(self, stream, assembler) -> None:
        self.reconnect_state = ReconnectState()
        current_stream = stream
        while True:
            try:
                async for result in current_stream:
                    now = self._now_ms()
                    self.reconnect_state.maybe_reset_on_idle(now)
                    if self.reconnect_state.consecutive_failures > 0:
                        self.reconnect_state.record_success()
                    for event in assembler.on_result(result):
                        adjusted = self._apply_offset(event, self.time_offset_ms)
                        if self.db is not None:
                            self.db.write(event, time_offset_ms=self.time_offset_ms)
                        await self._send(adjusted)
                return
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
            except ProviderResetError as exc:
                logger.exception("stt provider reset error in pump for call %s", self.call_id)
                self._reconnecting = True
                try:
                    next_stream = await self._handle_provider_reset(exc, current_stream)
                except ConnectionClosed:
                    return
                except sqlite3.DatabaseError as db_exc:
                    logger.exception("sqlite error while reconnecting for call %s", self.call_id)
                    await self._send_safe(
                        error_frame(self.call_id, "DB_WRITE_CONFLICT", {"reason": str(db_exc)})
                    )
                    return
                except Exception:
                    logger.exception(
                        "unexpected error while reconnecting for call %s", self.call_id
                    )
                    await self._send_safe(error_frame(self.call_id, "STT_STREAM_RESET"))
                    return
                finally:
                    self._reconnecting = False
                if next_stream is None:
                    return
                current_stream = next_stream
                continue
            except Exception:
                logger.exception("unexpected error in pump for call %s", self.call_id)
                await self._send(error_frame(self.call_id, "STT_STREAM_RESET"))
                return

    async def _handle_provider_reset(self, exc: Exception, current_stream):
        consecutive = self.reconnect_state.consecutive_failures + 1
        policy = load_reconnect_policy()
        if consecutive > policy.max_consecutive:
            await self._send_safe(
                error_frame(self.call_id, "STT_STREAM_RESET", {"attempts": consecutive})
            )
            await self._fail_meeting(exc)
            return None
        self.reconnect_state.record_failure(self._now_ms())
        backoff = self.reconnect_state.next_backoff_ms
        await self._send(error_frame(self.call_id, "STT_STREAM_RESET", {"attempt": consecutive}))
        await self._sleep(backoff / 1000)
        try:
            new_stream = await self.engine_factory(self._ctx()).start(self._ctx())
        except (
            ProviderResetError,
            ProviderAuthError,
            ConnectionClosed,
            OSError,
            TimeoutError,
        ):
            logger.exception("stt provider restart failed in pump for call %s", self.call_id)
            return current_stream
        elapsed_ms = self._now_ms() - self._stream_started_at_ms
        self.time_offset_ms += elapsed_ms
        if self.db is not None:
            self.db.write_meeting_started_update_offset(
                {"EventType": "START", "CallId": self.call_id},
                time_offset_ms=self.time_offset_ms,
                reconnect_attempts=consecutive,
            )
        self._stream_started_at_ms = self._now_ms()
        try:
            await current_stream.close()
        except (ProviderResetError, ProviderAuthError, ConnectionClosed, OSError):
            pass
        self.stream = new_stream
        return new_stream

    async def _fail_meeting(self, exc: Exception) -> None:
        logger.error("reconnect budget exhausted for call %s: %s", self.call_id, exc)
        if self.db is not None:
            self.db.write({"EventType": "END", "CallId": self.call_id})
            self.db.write_meeting_failed({"EventType": "FAILED", "CallId": self.call_id})
        try:
            await self.connection.close(1013, "stt-reconnect-exhausted")
        except Exception:
            logger.exception("failed to close connection for call %s", self.call_id)

    async def _reject_invalid_frame(self) -> None:
        await self._send(error_frame(self.call_id, INVALID_FRAME_CODE, INVALID_FRAME_CONTEXT))
        await self.connection.close(INVALID_FRAME_CLOSE_CODE, "invalid-frame")

    async def _send(self, event: dict) -> None:
        async with self.send_lock:
            await self.connection.send(serialize_event(event))

    async def _send_safe(self, event: dict) -> None:
        try:
            await self._send(event)
        except (ConnectionClosed, OSError, RuntimeError):
            logger.debug("dropped outbound frame for call %s: connection unavailable", self.call_id)
