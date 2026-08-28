from lma_pipeline.assembler import AssemblerConfig, SegmentAssembler
from lma_stt.types import Result, WordItem


def word(content, start, end, speaker=None):
    return WordItem(
        content=content,
        type="pronunciation",
        start_time=start,
        end_time=end,
        speaker=speaker,
    )


def event(call_id, segment_id, channel, speaker, start, end, text, partial):
    e = {
        "EventType": "ADD_TRANSCRIPT_SEGMENT",
        "CallId": call_id,
        "SegmentId": segment_id,
        "Channel": channel,
        "StartTime": start,
        "EndTime": end,
        "Transcript": text,
        "IsPartial": partial,
    }
    if speaker is not None:
        e["Speaker"] = speaker
    return e


def test_settled_window_emits_final_once_then_high_water_suppresses():
    asm = SegmentAssembler("call-1")
    p1 = asm.on_result(
        Result(
            result_id="r2",
            channel="CALLER",
            is_partial=True,
            items=[word("red", 0.0, 0.3), word("orange", 0.6, 1.0), word("blue", 1.2, 1.6)],
        )
    )
    assert p1 == [
        event("call-1", "r2-CALLER-w0-r0", "CALLER", None, 0.0, 1.6, "red orange blue", True)
    ]

    p2 = asm.on_result(
        Result(
            result_id="r2",
            channel="CALLER",
            is_partial=True,
            items=[
                word("red", 0.0, 0.3),
                word("orange", 0.6, 1.0),
                word("blue", 1.2, 1.6),
                word("green", 20.5, 20.9),
                word("yellow", 21.2, 21.6),
            ],
        )
    )
    assert p2 == [
        event("call-1", "r2-CALLER-w0-r0", "CALLER", None, 0.0, 1.6, "red orange blue", False),
        event("call-1", "r2-CALLER-w1-r0", "CALLER", None, 20.5, 21.6, "green yellow", True),
    ]

    p3 = asm.on_result(
        Result(
            result_id="r2",
            channel="CALLER",
            is_partial=True,
            items=[
                word("red", 0.0, 0.3),
                word("orange", 0.6, 1.0),
                word("blue", 1.2, 1.6),
                word("green", 20.5, 20.9),
                word("yellow", 21.2, 21.6),
                word("purple", 22.0, 22.4),
            ],
        )
    )
    assert p3 == [
        event("call-1", "r2-CALLER-w1-r0", "CALLER", None, 20.5, 22.4, "green yellow purple", True)
    ]

    shrunk = asm.on_result(
        Result(
            result_id="r2",
            channel="CALLER",
            is_partial=True,
            items=[word("red", 0.0, 0.3), word("orange", 0.6, 1.0), word("blue", 1.2, 1.6)],
        )
    )
    assert shrunk == []

    final = asm.on_result(
        Result(
            result_id="r2",
            channel="CALLER",
            is_partial=False,
            items=[
                word("red", 0.0, 0.3, "spk_0"),
                word("orange", 0.6, 1.0, "spk_0"),
                word("blue", 1.2, 1.6, "spk_0"),
                word("green", 20.5, 20.9, "spk_1"),
                word("yellow", 21.2, 21.6, "spk_1"),
                word("purple", 22.0, 22.4, "spk_1"),
            ],
        )
    )
    assert final == [
        event("call-1", "r2-CALLER-w0-r0", "CALLER", "spk_0", 0.0, 1.6, "red orange blue", False),
        event(
            "call-1", "r2-CALLER-w1-r0", "CALLER", "spk_1", 20.5, 22.4, "green yellow purple", False
        ),
    ]


def test_anchor_memoized_so_revised_timestamps_cannot_migrate_items():
    asm = SegmentAssembler("call-1")
    first = asm.on_result(
        Result(
            result_id="r9",
            channel="CALLER",
            is_partial=True,
            items=[
                word("aa", 10.0, 10.3),
                word("bb", 19.9, 20.2),
                word("cc", 29.95, 30.2),
                word("dd", 30.4, 30.7),
            ],
        )
    )
    assert first == [
        event("call-1", "r9-CALLER-w0-r0", "CALLER", None, 10.0, 30.2, "aa bb cc", True)
    ]

    revised = asm.on_result(
        Result(
            result_id="r9",
            channel="CALLER",
            is_partial=True,
            items=[
                word("aa", 9.0, 9.3),
                word("bb", 19.9, 20.2),
                word("cc", 29.95, 30.2),
                word("dd", 30.4, 30.7),
            ],
        )
    )
    assert revised == [
        event("call-1", "r9-CALLER-w0-r0", "CALLER", None, 9.0, 30.2, "aa bb cc", False),
        event("call-1", "r9-CALLER-w1-r0", "CALLER", None, 30.4, 30.7, "dd", True),
    ]


def test_new_result_resets_anchor_and_high_water():
    asm = SegmentAssembler("call-1")
    asm.on_result(
        Result(
            result_id="r1",
            channel="CALLER",
            is_partial=False,
            items=[word("aa", 0.0, 0.3, "spk_0")],
        )
    )
    out = asm.on_result(
        Result(
            result_id="r2",
            channel="CALLER",
            is_partial=True,
            items=[word("bb", 100.0, 100.3), word("cc", 130.0, 130.3)],
        )
    )
    assert [e["SegmentId"] for e in out] == ["r2-CALLER-w0-r0", "r2-CALLER-w1-r0"]
    assert [e["IsPartial"] for e in out] == [False, True]


def test_windowing_disabled_follows_engine_boundaries():
    asm = SegmentAssembler("call-1", AssemblerConfig(max_segment_seconds=0.0))
    out = asm.on_result(
        Result(
            result_id="r3",
            channel="CALLER",
            is_partial=False,
            items=[word("aa", 0.0, 0.3, "spk_0"), word("zz", 44.0, 44.3, "spk_0")],
        )
    )
    assert [e["SegmentId"] for e in out] == ["r3-CALLER-w0-r0"]
    assert out[0]["EndTime"] == 44.3
