import asyncio
import io
import math
import re
import struct
import time

READY_LINE = re.compile(r"^SIDECAR_READY port=(?P<port>\d+) token=(?P<token>[0-9a-f]{32})\n$")


class ScriptedStream:
    def __init__(self, results: list[dict]):
        self.results = list(results)
        self.fed: list[bytes] = []
        self.closed = False
        self.queue: asyncio.Queue = asyncio.Queue()

    async def feed(self, pcm: bytes) -> None:
        self.fed.append(pcm)
        if self.results:
            await self.queue.put(self.results.pop(0))

    async def close(self) -> None:
        self.closed = True
        while self.results:
            await self.queue.put(self.results.pop(0))
        await self.queue.put(None)

    def __aiter__(self):
        return self

    async def __anext__(self) -> dict:
        item = await self.queue.get()
        if item is None:
            raise StopAsyncIteration
        return item


class ScriptedEngine:
    def __init__(self, results: list[dict]):
        self.results = results
        self.started_with: list[dict] = []
        self.stream: ScriptedStream | None = None

    async def start(self, ctx: dict) -> ScriptedStream:
        self.started_with.append(dict(ctx))
        self.stream = ScriptedStream(list(self.results))
        return self.stream


class MemoryConnection:
    def __init__(self):
        self.sent: list[str] = []
        self.closes: list[tuple[int, str]] = []
        self.open = True

    async def send(self, message: str) -> None:
        if not self.open:
            raise RuntimeError("connection closed")
        self.sent.append(message)

    async def close(self, code: int, reason: str = "") -> None:
        self.open = False
        self.closes.append((code, reason))


async def eventually(predicate, timeout: float = 2.0) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return
        await asyncio.sleep(0.01)
    raise AssertionError("condition not met before timeout")


def sine_chunk(sample_rate: int, caller_freq: int, agent_freq: int) -> bytes:
    count = int(sample_rate * 0.1)
    frames = bytearray()
    for i in range(count):
        left = int(2000 * math.sin(2 * math.pi * caller_freq * i / sample_rate))
        right = int(2000 * math.sin(2 * math.pi * agent_freq * i / sample_rate))
        frames += struct.pack("<hh", left, right)
    return bytes(frames)


async def spawn_sidecar(engine_factory, *, record_meeting: bool = False):
    from sidecar.server import run_server

    sink = io.StringIO()
    stop = asyncio.Event()
    task = asyncio.create_task(
        run_server(
            engine_factory,
            stop=stop,
            ready_sink=sink,
            record_meeting=record_meeting,
        )
    )
    await eventually(lambda: "SIDECAR_READY" in sink.getvalue())
    match = READY_LINE.fullmatch(sink.getvalue())
    assert match is not None
    return stop, task, int(match.group("port")), match.group("token")
