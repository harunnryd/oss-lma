import asyncio
import json

import pytest

from lma_stt.deepgram import DeepgramConfig, DeepgramEngine
from lma_stt.tests.transports import FakeTransport, fake_connect
from lma_stt.types import ProviderAuthError, ProviderResetError

CTX = {
    "call_id": "m-test",
    "sample_rate": 16000,
    "diarize": {"system": True, "mic": True},
    "language_hints": [],
}


def test_start_opens_connection_with_token_header_and_ctx_sample_rate():
    conn = FakeTransport()
    capture: dict = {}

    async def run():
        engine = DeepgramEngine(
            DeepgramConfig(api_key="tok-123"), connect=fake_connect(conn, capture)
        )
        stream = await engine.start(CTX)
        await stream.feed(b"\x01" * 32)
        await stream.close()

    asyncio.run(run())
    assert capture["url"].startswith("wss://api.deepgram.com/v1/listen?")
    assert "encoding=linear16" in capture["url"]
    assert "multichannel=true" in capture["url"]
    assert "channels=2" in capture["url"]
    assert "sample_rate=16000" in capture["url"]
    assert capture["headers"] == {"Authorization": "Token tok-123"}
    assert b"\x01" * 32 in conn.sent
    assert json.loads(conn.sent[-1]) == {"type": "CloseStream"}
    assert conn.closed is True


@pytest.mark.parametrize("status", [401, 403])
def test_handshake_401_and_403_raise_provider_auth_error(status):
    async def run():
        engine = DeepgramEngine(
            DeepgramConfig(api_key="k"),
            connect=fake_connect(FakeTransport(status_code=status)),
        )
        await engine.start(CTX)

    with pytest.raises(ProviderAuthError):
        asyncio.run(run())


def test_other_handshake_failures_raise_provider_reset_error():
    async def run():
        engine = DeepgramEngine(
            DeepgramConfig(api_key="k"),
            connect=fake_connect(FakeTransport(status_code=500)),
        )
        await engine.start(CTX)

    with pytest.raises(ProviderResetError):
        asyncio.run(run())


@pytest.mark.parametrize(
    "failure", [ConnectionResetError("peer closed"), RuntimeError("socket gone")]
)
def test_post_handshake_transport_failure_raises_provider_reset_error(failure):
    conn = FakeTransport(error=failure)

    async def run():
        engine = DeepgramEngine(DeepgramConfig(api_key="k"), connect=fake_connect(conn))
        stream = await engine.start(CTX)
        await stream.feed(b"\x00" * 32)
        await anext(stream)

    with pytest.raises(ProviderResetError):
        asyncio.run(run())


def test_provider_error_frame_raises_provider_reset_error_with_description():
    conn = FakeTransport(messages=[json.dumps({"type": "Error", "description": "Invalid model"})])

    async def run():
        engine = DeepgramEngine(DeepgramConfig(api_key="k"), connect=fake_connect(conn))
        stream = await engine.start(CTX)
        await anext(stream)

    with pytest.raises(ProviderResetError, match="Invalid model"):
        asyncio.run(run())
