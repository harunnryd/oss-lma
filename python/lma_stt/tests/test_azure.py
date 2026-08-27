import asyncio
import json

from lma_stt.azure import AzureConfig, AzureEngine, AzureResultStream, map_messages
from lma_stt.fixtures import load_fixture
from lma_stt.tests.transports import FakeTransport


def test_azure_detailed_phrase_maps_ticks_and_attached_punctuation():
    messages, expected = load_fixture("azure", "phrases")
    assert map_messages(messages, channel="AGENT") == expected


def test_azure_hypothesis_maps_a_partial_result():
    message = {"Offset": 10_000_000, "Duration": 2_000_000, "Text": "hello"}
    stream = AzureResultStream(
        FakeTransport(messages=["Path: speech.hypothesis\r\n\r\n" + json.dumps(message)]),
        "CALLER",
        16_000,
    )
    result = asyncio.run(anext(stream))
    assert result["is_partial"] is True
    assert result["items"][0].content == "hello"
    assert result["items"][0].channel == "CALLER"


def test_azure_opens_two_sessions_and_frames_each_downsampled_channel():
    caller, agent = FakeTransport(), FakeTransport()
    connections = iter([caller, agent])

    async def connect(_url, _headers):
        return next(connections)

    async def run():
        stream = await AzureEngine(AzureConfig("key", "eastus"), connect=connect).start(
            {"call_id": "id", "sample_rate": 48_000, "diarize": {}, "language_hints": []}
        )
        await stream.feed(b"\x01\x00\x11\x00" * 3)

    asyncio.run(run())
    for conn, sample in ((caller, b"\x01\x00"), (agent, b"\x11\x00")):
        assert "Path: speech.config" in conn.sent[0]
        audio = conn.sent[1]
        assert isinstance(audio, bytes)
        assert b"Path: audio\r\n" in audio
        assert b"Content-Type: audio/x-wav\r\n" in audio
        assert b"RIFF" in audio and b"WAVE" in audio
        assert audio.endswith(sample)
