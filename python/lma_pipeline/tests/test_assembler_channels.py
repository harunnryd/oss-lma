from lma_pipeline.assembler import SegmentAssembler
from lma_stt.types import Result, WordItem


def word(content, start, end, speaker=None):
    return WordItem(
        content=content,
        type="pronunciation",
        start_time=start,
        end_time=end,
        speaker=speaker,
    )


def test_interleaved_channels_keep_ids_windows_and_text_separate():
    asm = SegmentAssembler("call-4")

    caller1 = asm.on_result(
        Result(
            result_id="c1",
            channel="CALLER",
            is_partial=False,
            items=[word("caller", 0.0, 0.4, "spk_0"), word("here", 0.5, 0.8, "spk_0")],
        )
    )
    agent1 = asm.on_result(
        Result(
            result_id="a1",
            channel="AGENT",
            is_partial=False,
            items=[word("agent", 0.5, 0.9, "spk_3"), word("ready", 1.0, 1.3, "spk_3")],
        )
    )
    caller2 = asm.on_result(
        Result(
            result_id="c2",
            channel="CALLER",
            is_partial=False,
            items=[
                word("are", 19.0, 19.3, "spk_0"),
                word("you", 19.6, 19.9, "spk_0"),
                word("hearing", 39.4, 39.7, "spk_0"),
                word("me", 39.9, 40.2, "spk_0"),
            ],
        )
    )
    agent2 = asm.on_result(
        Result(
            result_id="a2",
            channel="AGENT",
            is_partial=False,
            items=[
                word("loud", 20.4, 20.7, "spk_3"),
                word("and", 20.8, 21.0, "spk_3"),
                word("clear", 21.2, 21.5, "spk_3"),
            ],
        )
    )

    assert [e["SegmentId"] for e in caller1 + caller2] == [
        "c1-CALLER-w0-r0",
        "c2-CALLER-w0-r0",
        "c2-CALLER-w1-r0",
    ]
    assert [e["SegmentId"] for e in agent1 + agent2] == [
        "a1-AGENT-w0-r0",
        "a2-AGENT-w0-r0",
    ]
    assert all(e["Channel"] == "CALLER" for e in caller1 + caller2)
    assert all(e["Channel"] == "AGENT" for e in agent1 + agent2)

    caller_text = " ".join(e["Transcript"] for e in caller1 + caller2)
    agent_text = " ".join(e["Transcript"] for e in agent1 + agent2)
    for foreign in ("agent", "ready", "loud", "clear"):
        assert foreign not in caller_text
    for foreign in ("caller", "here", "hearing"):
        assert foreign not in agent_text

    assert all(e["CallId"] == "call-4" for e in caller1 + caller2 + agent1 + agent2)
