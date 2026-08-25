from lma_pipeline.assembler import AssemblerConfig, SegmentAssembler
from lma_stt.types import WordItem


def word(content, start, end, speaker=None):
    return WordItem(
        content=content, type="pronunciation",
        start_time=start, end_time=end, speaker=speaker,
    )


def punct(content, start):
    return WordItem(
        content=content, type="punctuation",
        start_time=start, end_time=start, speaker=None,
    )


def assembler(**overrides):
    return SegmentAssembler("call-1", AssemblerConfig(**overrides))


def test_buckets_split_at_twenty_second_boundaries():
    asm = assembler()
    items = [word("aa", 10.0, 10.3), word("bb", 19.9, 20.2),
             word("cc", 29.95, 30.2), word("dd", 30.4, 30.7)]
    buckets = asm._buckets(items, 10.0)
    assert [index for index, _ in buckets] == [0, 1]
    assert [i.content for i in buckets[0][1]] == ["aa", "bb", "cc"]
    assert [i.content for i in buckets[1][1]] == ["dd"]


def test_punctuation_rides_preceding_word_window():
    asm = assembler()
    items = [word("aa", 0.0, 0.3), punct(",", 0.4),
             word("bb", 19.9, 20.2), word("cc", 20.05, 20.3)]
    buckets = asm._buckets(items, 0.0)
    assert [i.content for i in buckets[0][1]] == ["aa", ",", "bb"]
    assert [i.content for i in buckets[1][1]] == ["cc"]


def test_bucketing_depends_only_on_passed_origin():
    asm = assembler()
    original = [word("aa", 10.0, 10.3), word("bb", 19.9, 20.2),
                word("cc", 29.95, 30.2), word("dd", 30.4, 30.7)]
    revised = [word("aa", 9.0, 9.3), word("bb", 19.9, 20.2),
               word("cc", 29.95, 30.2), word("dd", 30.4, 30.7)]
    pinned = asm._buckets(revised, 10.0)
    assert [index for index, _ in pinned] == [0, 1]
    drifted = asm._buckets(revised, 9.0)
    assert [i.content for i in drifted[1][1]] == ["cc", "dd"]


def test_origin_prefers_first_pronunciation():
    asm = assembler()
    items = [punct(",", 3.0), word("aa", 10.0, 10.3)]
    assert asm._origin(items) == 10.0
    assert asm._origin([punct(",", 3.0)]) == 3.0


def test_zero_limit_disables_windowing():
    asm = assembler(max_segment_seconds=0.0)
    items = [word("aa", 0.0, 0.3), word("zz", 44.0, 44.3)]
    buckets = asm._buckets(items, 0.0)
    assert len(buckets) == 1
    assert buckets[0][0] == 0
    assert len(buckets[0][1]) == 2
