import asyncio
import json

from lma_stt.deepgram import DeepgramConfig, DeepgramEngine
from lma_stt.tests.transports import FakeTransport, fake_connect

CTX = {
    "call_id": "m-test",
    "sample_rate": 16000,
    "diarize": {"system": True, "mic": True},
    "language_hints": [],
}

KEEPALIVE_FRAME = json.dumps({"type": "KeepAlive"})


class FakeClock:
    def __init__(self):
        self.now = 0.0

    def __call__(self) -> float:
        return self.now

    async def sleep(self, seconds: float) -> None:
        self.now += seconds
        await asyncio.sleep(0)


def keepalives(sent: list) -> list[str]:
    return [f for f in sent if isinstance(f, str) and json.loads(f).get("type") == "KeepAlive"]


def make_engine(conn: FakeTransport, clock: FakeClock) -> DeepgramEngine:
    return DeepgramEngine(
        DeepgramConfig(api_key="k"),
        connect=fake_connect(conn),
        clock=clock,
        sleep=clock.sleep,
        interval_s=5.0,
    )


def test_keepalive_flows_once_after_six_seconds_of_silence():
    conn = FakeTransport()
    clock = FakeClock()

    async def run():
        engine = make_engine(conn, clock)
        stream = await engine.start(CTX)
        while clock.now < 6.0:
            await asyncio.sleep(0)
        await stream.close()
        return keepalives(conn.sent)

    assert asyncio.run(run()) == [KEEPALIVE_FRAME]


def test_active_audio_suppresses_keepalive():
    conn = FakeTransport()
    clock = FakeClock()

    async def run():
        engine = make_engine(conn, clock)
        stream = await engine.start(CTX)
        for _ in range(16):
            await asyncio.sleep(0)
            await stream.feed(b"\x00" * 32)
        await stream.close()
        return keepalives(conn.sent)

    assert asyncio.run(run()) == []


def test_close_cancels_keepalive_task():
    conn = FakeTransport()
    clock = FakeClock()

    async def run():
        engine = make_engine(conn, clock)
        stream = await engine.start(CTX)
        task = stream.keepalive_task
        await stream.close()
        return task

    assert asyncio.run(run()).done() is True
