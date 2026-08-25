import asyncio

from lma_pipeline import SegmentAssembler
from lma_stt.types import MeetingContext, ProviderAuthError, ProviderResetError
from websockets.exceptions import ConnectionClosed

from sidecar.frames import (
    INVALID_FRAME_CLOSE_CODE,
    INVALID_FRAME_CODE,
    INVALID_FRAME_CONTEXT,
    FrameError,
    Start,
    error_frame,
    parse_frame,
    serialize_event,
)

DRAIN_TIMEOUT_SECONDS = 10.0


class Session:
    def __init__(self, connection, engine_factory):
        self.connection = connection
        self.engine_factory = engine_factory
        self.call_id = ""
        self.stream = None
        self.assembler = None
        self.pump_task: asyncio.Task | None = None
        self.paused = False
        self.chunk_bytes = 0
        self.send_lock = asyncio.Lock()

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
        await self.stream.feed(pcm)

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
        self.assembler = SegmentAssembler(frame.call_id)
        self.pump_task = asyncio.create_task(self._pump())

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
            await stream.close()
            await asyncio.wait_for(pump_task, DRAIN_TIMEOUT_SECONDS)
            return
        if pump_task is not None:
            pump_task.cancel()
            await asyncio.gather(pump_task, return_exceptions=True)
        try:
            await stream.close()
        except (ProviderResetError, ProviderAuthError, ConnectionClosed, OSError):
            pass

    async def _pump(self) -> None:
        stream = self.stream
        assembler = self.assembler
        try:
            async for result in stream:
                for event in assembler.on_result(result):
                    await self._send(event)
        except ConnectionClosed:
            return

    async def _reject_invalid_frame(self) -> None:
        await self._send(error_frame(self.call_id, INVALID_FRAME_CODE, INVALID_FRAME_CONTEXT))
        await self.connection.close(INVALID_FRAME_CLOSE_CODE, "invalid-frame")

    async def _send(self, event: dict) -> None:
        async with self.send_lock:
            await self.connection.send(serialize_event(event))
