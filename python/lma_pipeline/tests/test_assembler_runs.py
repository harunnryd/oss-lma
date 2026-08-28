from lma_pipeline.assembler import SegmentAssembler
from lma_stt.types import WordItem


def word(content, start, end, speaker=None):
    return WordItem(
        content=content,
        type="pronunciation",
        start_time=start,
        end_time=end,
        speaker=speaker,
    )


def punct(content, start):
    return WordItem(
        content=content,
        type="punctuation",
        start_time=start,
        end_time=start,
        speaker=None,
    )


def test_contiguous_same_label_items_form_one_run():
    asm = SegmentAssembler("call-1")
    items = [
        word("aa", 0.0, 0.3, "spk_0"),
        word("bb", 0.4, 0.7, "spk_0"),
        word("cc", 1.0, 1.3, "spk_1"),
        word("dd", 1.4, 1.7, "spk_1"),
        word("ee", 2.0, 2.3, "spk_0"),
    ]
    runs = asm._runs(items)
    assert [run["label"] for run in runs] == ["spk_0", "spk_1", "spk_0"]
    assert [run["words"] for run in runs] == [2, 2, 1]


def test_unlabelled_item_attaches_to_current_run():
    asm = SegmentAssembler("call-1")
    items = [word("aa", 0.0, 0.3, "spk_0"), punct(",", 0.35), word("bb", 0.4, 0.7, "spk_0")]
    runs = asm._runs(items)
    assert len(runs) == 1
    assert runs[0]["label"] == "spk_0"
    assert runs[0]["words"] == 2
    assert runs[0]["start"] == 0.0
    assert runs[0]["end"] == 0.7


def test_boundary_punctuation_stays_with_previous_speaker():
    asm = SegmentAssembler("call-1")
    items = [word("aa", 0.0, 0.3, "spk_0"), punct(",", 0.35), word("bb", 1.0, 1.3, "spk_1")]
    runs = asm._runs(items)
    assert [run["label"] for run in runs] == ["spk_0", "spk_1"]
    assert [i.content for i in runs[0]["items"]] == ["aa", ","]
    assert [i.content for i in runs[1]["items"]] == ["bb"]


def test_leading_unlabelled_adopts_first_label():
    asm = SegmentAssembler("call-1")
    items = [punct('"', 0.0), word("aa", 0.1, 0.4, "spk_0"), word("bb", 0.5, 0.8, "spk_0")]
    runs = asm._runs(items)
    assert len(runs) == 1
    assert runs[0]["label"] == "spk_0"
    assert [i.content for i in runs[0]["items"]] == ['"', "aa", "bb"]
