import asyncio
import json

from lma_stt.assemblyai import AssemblyAIConfig, AssemblyAIEngine, map_messages
from lma_stt.fixtures import load_fixture
from lma_stt.tests.transports import FakeTransport


def test_assemblyai_turn_partial_and_final_keep_result_id_and_channel():
    messages, expected = load_fixture("assemblyai", "turns")
    assert map_messages(messages, channel="CALLER") == expected


def test_assemblyai_multiplexer_returns_all_simultaneously_ready_channel_results():
    caller = FakeTransport(
        messages=[
            json.dumps({"type": "Begin", "id": "c"}),
            json.dumps({"type": "Turn", "turn_order": 1, "end_of_turn": True}),
        ]
    )
    agent = FakeTransport(
        messages=[
            json.dumps({"type": "Begin", "id": "a"}),
            json.dumps({"type": "Turn", "turn_order": 1, "end_of_turn": True}),
        ]
    )
    connections = iter([caller, agent])

    async def connect(_url, _headers):
        return next(connections)

    async def run():
        stream = await AssemblyAIEngine(AssemblyAIConfig("key"), connect=connect).start(
            {"call_id": "id", "sample_rate": 16_000, "diarize": {}, "language_hints": []}
        )
        return [await anext(stream), await anext(stream)]

    assert [result["items"] for result in asyncio.run(run())] == [[], []]
