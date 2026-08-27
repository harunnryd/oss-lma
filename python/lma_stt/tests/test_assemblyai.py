from lma_stt.assemblyai import map_messages
from lma_stt.fixtures import load_fixture


def test_assemblyai_turn_partial_and_final_keep_result_id_and_channel():
    messages, expected = load_fixture("assemblyai", "turns")
    assert map_messages(messages, channel="CALLER") == expected
