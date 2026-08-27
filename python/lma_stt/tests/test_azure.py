from lma_stt.azure import map_messages
from lma_stt.fixtures import load_fixture


def test_azure_detailed_phrase_maps_ticks_and_attached_punctuation():
    messages, expected = load_fixture("azure", "phrases")
    assert map_messages(messages, channel="AGENT") == expected
