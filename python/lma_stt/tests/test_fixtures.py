from lma_stt.fixtures import load_fixture


def test_loads_messages_and_expected_results_pair():
    messages, expected = load_fixture("deepgram", "two_channel_miniature")
    assert len(messages) == 3
    assert all(m["type"] == "Results" for m in messages)
    assert messages[0]["metadata"]["sequence"] == messages[1]["metadata"]["sequence"]
    assert messages[2]["metadata"]["sequence"] == 2
    assert len(expected) == 3
    assert expected[0]["result_id"] == "req-mini-1-1"
    assert expected[0]["is_final"] is False
    assert expected[1]["result_id"] == expected[0]["result_id"]
    assert expected[1]["is_final"] is True
    assert expected[2]["result_id"] == "req-mini-1-2"
    assert all(i["channel"] == "AGENT" for i in expected[2]["items"])
