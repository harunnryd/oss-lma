import asyncio

import pytest

from lma_stt.fake import CHUNK_BYTES, FakeEngine, two_speaker_script
from lma_stt.types import ProviderAuthError, ProviderResetError

CTX = {
    "call_id": "m-1",
    "sample_rate": 48000,
    "diarize": {"system": True, "mic": True},
    "language_hints": [],
}


def drain(chunks: int, **kwargs) -> list:
    async def run():
        engine = FakeEngine(**kwargs)
        stream = await engine.start(CTX)
        for _ in range(chunks):
            await stream.feed(b"\x00" * CHUNK_BYTES)
        await stream.close()
        results = [item async for item in stream]
        return results

    return asyncio.run(run())


def test_two_speaker_script_partial_then_final():
    results = drain(25, script=two_speaker_script())
    assert [r["result_id"] for r in results] == ["fake-r0", "fake-r0"]
    assert [r["is_partial"] for r in results] == [True, False]
    partial = results[0]
    assert [i.content for i in partial["items"]] == [
        "can",
        "everyone",
        "see",
        "the",
        "updated",
        "forecast",
    ]
    assert all(i.speaker is None for i in partial["items"])
    assert all(i.channel == "CALLER" for i in partial["items"])
    assert all(i.type == "pronunciation" for i in partial["items"])
    assert [i.start_time for i in partial["items"]] == [1.4, 1.5, 1.6, 1.7, 1.8, 1.9]
    assert [i.end_time for i in partial["items"]] == [1.5, 1.6, 1.7, 1.8, 1.9, 2.0]
    assert {i.result_id for i in partial["items"]} == {"fake-r0"}


def test_finals_carry_labels_for_whole_grouped_result():
    results = drain(50, script=two_speaker_script())
    caller_final = results[1]
    agent_partial = results[2]
    agent_final = results[3]
    assert len(caller_final["items"]) == 6
    assert [i.speaker for i in caller_final["items"]] == ["spk_0"] * 6
    assert [i.content for i in agent_partial["items"]] == [
        "yes",
        "and",
        "the",
        "variance",
        "explains",
        "the",
        "hiring",
        "delay",
    ]
    assert all(i.speaker is None for i in agent_partial["items"])
    assert all(i.channel == "AGENT" for i in agent_partial["items"])
    assert [i.speaker for i in agent_final["items"]] == ["spk_1"] * 8
    assert agent_final["items"][0].start_time == 3.2


def test_rejects_wrong_chunk_size():
    async def run():
        engine = FakeEngine(script=[])
        stream = await engine.start(CTX)
        await stream.feed(b"\x00" * 1024)

    with pytest.raises(ValueError):
        asyncio.run(run())


def test_auth_failure_raises_on_start():
    async def run():
        engine = FakeEngine(script=[], auth_failure=True)
        await engine.start(CTX)

    with pytest.raises(ProviderAuthError):
        asyncio.run(run())


def test_reset_raises_mid_stream_after_scripted_chunk_count():
    async def run():
        engine = FakeEngine(script=two_speaker_script(), reset_after_chunks=3)
        stream = await engine.start(CTX)
        for _ in range(3):
            await stream.feed(b"\x00" * CHUNK_BYTES)

    with pytest.raises(ProviderResetError):
        asyncio.run(run())


def test_reset_before_first_stage_emits_no_results():
    async def run():
        engine = FakeEngine(script=two_speaker_script(), reset_after_chunks=10)
        stream = await engine.start(CTX)
        with pytest.raises(ProviderResetError):
            for _ in range(10):
                await stream.feed(b"\x00" * CHUNK_BYTES)
        return list(stream._pending)

    assert asyncio.run(run()) == []
