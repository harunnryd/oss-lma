from lma_pipeline.assembler import SegmentAssembler
from lma_stt.types import Result, WordItem


def word(content, start, end, speaker=None):
    return WordItem(
        content=content, type="pronunciation",
        start_time=start, end_time=end, speaker=speaker,
    )


def result(result_id, items, is_partial=True, channel="AGENT"):
    return Result(result_id=result_id, channel=channel,
                  is_partial=is_partial, items=items)


def test_unlabelled_items_bin_against_declared_active_speaker():
    asm = SegmentAssembler("call-3")
    asm.set_active_speaker("AGENT", "Ayu")
    out = asm.on_result(result("ra", [word("checking", 5.0, 5.3),
                                      word("your", 5.5, 5.7),
                                      word("mic", 5.9, 6.1)]))
    assert len(out) == 1
    assert out[0]["SegmentId"] == "Ayu-5000-AGENT"
    assert out[0]["Speaker"] == "Ayu"
    assert out[0]["Transcript"] == "checking your mic"
    assert (out[0]["StartTime"], out[0]["EndTime"]) == (5.0, 6.1)
    assert out[0]["IsPartial"] is True
    assert out[0]["EventType"] == "ADD_TRANSCRIPT_SEGMENT"
    assert out[0]["CallId"] == "call-3"
    assert out[0]["Channel"] == "AGENT"


def test_mid_utterance_speaker_change_starts_new_segment_not_mutation():
    asm = SegmentAssembler("call-3")
    asm.set_active_speaker("AGENT", "Ayu")
    first = asm.on_result(result("ra", [word("checking", 5.0, 5.3),
                                        word("your", 5.5, 5.7),
                                        word("mic", 5.9, 6.1)]))
    snapshot = dict(first[0])
    asm.set_active_speaker("AGENT", "Bea")
    second = asm.on_result(result("rb", [word("thanks", 9.0, 9.3),
                                         word("bea", 9.5, 9.7),
                                         word("here", 9.9, 10.1)]))
    assert len(second) == 1
    assert second[0]["SegmentId"] == "Bea-9000-AGENT"
    assert second[0]["Speaker"] == "Bea"
    assert first[0] == snapshot
    assert "checking" not in second[0]["Transcript"]

    closing = asm.on_result(result("rc", [word("checking", 5.0, 5.3),
                                          word("your", 5.5, 5.7),
                                          word("mic", 5.9, 6.1),
                                          word("again", 6.2, 6.5)],
                                   is_partial=False))
    assert len(closing) == 1
    assert closing[0]["SegmentId"] == "Ayu-5000-AGENT"
    assert closing[0]["IsPartial"] is False
    assert closing[0]["Transcript"] == "checking your mic again"


def test_same_speaker_across_results_extends_one_segment():
    asm = SegmentAssembler("call-3")
    asm.set_active_speaker("AGENT", "Bea")
    asm.on_result(result("rb", [word("thanks", 9.0, 9.3)]))
    out = asm.on_result(result("rd", [word("thanks", 12.0, 12.3),
                                      word("again", 12.5, 12.7)]))
    assert len(out) == 1
    assert out[0]["SegmentId"] == "Bea-9000-AGENT"
    assert out[0]["Transcript"] == "thanks thanks again"
    assert out[0]["EndTime"] == 12.7


def test_channel_without_any_active_speaker_declaration_stays_diarized_default():
    asm = SegmentAssembler("call-3")
    out = asm.on_result(result("re", [word("hello", 0.5, 0.8)],
                               channel="CALLER"))
    assert out[0]["SegmentId"] == "re-CALLER-w0-r0"
    assert out[0]["Speaker"] is None


def test_labelled_result_flips_channel_to_diarized_permanently():
    asm = SegmentAssembler("call-3")
    asm.set_active_speaker("AGENT", "Ayu")
    asm.on_result(result("ra", [word("checking", 5.0, 5.3)]))
    out = asm.on_result(Result(
        result_id="rf", channel="AGENT", is_partial=False,
        items=[word("alpha", 20.0, 20.3, "spk_2"),
               word("beta", 20.5, 20.8, "spk_2"),
               word("gamma", 21.0, 21.3, "spk_2")],
    ))
    assert out[0]["SegmentId"] == "rf-AGENT-w0-r0"
    assert out[0]["Speaker"] == "spk_2"
