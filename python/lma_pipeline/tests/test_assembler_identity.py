import re

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


OPENING = [("how", 0.0, 0.3), ("are", 0.4, 0.6), ("you", 0.7, 1.1)]

WINDOW1 = [
    ("what", 21.0, 21.2),
    ("did", 21.3, 21.5),
    ("you", 21.6, 21.8),
    ("need", 21.9, 22.1),
    ("well", 22.5, 22.65),
    ("status", 23.0, 23.2),
    ("update", 23.3, 23.5),
    ("please", 23.6, 23.8),
    ("now", 23.9, 24.1),
    ("sure", 24.5, 24.7),
    ("here", 25.0, 25.2),
    ("it", 25.3, 25.5),
    ("is", 25.6, 25.8),
    ("then", 25.9, 26.2),
]

WINDOW1_LABELS = [
    "spk_0",
    "spk_0",
    "spk_0",
    "spk_0",
    "spk_1",
    "spk_1",
    "spk_1",
    "spk_1",
    "spk_1",
    "spk_0",
    "spk_0",
    "spk_0",
    "spk_0",
    "spk_0",
]


def labelled_window1():
    return [
        word(content, start, end, label)
        for (content, start, end), label in zip(WINDOW1, WINDOW1_LABELS)
    ]


def test_every_diarized_id_matches_the_grammar():
    asm = SegmentAssembler("call-1")
    out = asm.on_result(
        Result(
            result_id="r4",
            channel="AGENT",
            is_partial=False,
            items=[
                word("aa", 0.0, 0.3, "spk_0"),
                word("bb", 0.5, 0.8, "spk_0"),
                word("cc", 25.0, 25.3, "spk_1"),
                word("dd", 25.5, 25.8, "spk_1"),
            ],
        )
    )
    pattern = re.compile(r"^r4-AGENT-w\d+-r\d+$")
    assert [e["SegmentId"] for e in out] == ["r4-AGENT-w0-r0", "r4-AGENT-w1-r0"]
    assert all(pattern.match(e["SegmentId"]) for e in out)


def test_r7_worked_example_partial_to_final_stability():
    asm = SegmentAssembler("call-2")
    opening = [word(content, start, end) for content, start, end in OPENING]

    p1 = asm.on_result(Result(result_id="r7", channel="CALLER", is_partial=True, items=opening))
    assert [e["SegmentId"] for e in p1] == ["r7-CALLER-w0-r0"]
    assert p1[0]["IsPartial"] is True
    assert "Speaker" not in p1[0]

    p2 = asm.on_result(
        Result(
            result_id="r7",
            channel="CALLER",
            is_partial=True,
            items=opening + [word(c, s, e) for c, s, e in WINDOW1],
        )
    )
    assert [(e["SegmentId"], e["IsPartial"]) for e in p2] == [
        ("r7-CALLER-w0-r0", False),
        ("r7-CALLER-w1-r0", True),
    ]
    assert all("Speaker" not in e for e in p2)

    final = asm.on_result(
        Result(
            result_id="r7",
            channel="CALLER",
            is_partial=False,
            items=[word(content, start, end, "spk_0") for content, start, end in OPENING]
            + labelled_window1(),
        )
    )
    assert [e["SegmentId"] for e in final] == [
        "r7-CALLER-w0-r0",
        "r7-CALLER-w1-r0",
        "r7-CALLER-w1-r1",
        "r7-CALLER-w1-r2",
    ]

    r2 = final[3]
    assert r2["SegmentId"] == "r7-CALLER-w1-r2"
    assert r2["Speaker"] == "spk_0"
    assert r2["Transcript"] == "here it is then"
    assert (r2["StartTime"], r2["EndTime"]) == (25.0, 26.2)
    assert r2["IsPartial"] is False

    assert p1[0]["SegmentId"] == final[0]["SegmentId"] == "r7-CALLER-w0-r0"
    assert p2[1]["SegmentId"] == final[1]["SegmentId"] == "r7-CALLER-w1-r0"
    assert final[0]["Speaker"] == "spk_0"
    assert final[0]["IsPartial"] is False
