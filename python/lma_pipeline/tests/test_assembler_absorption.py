from lma_pipeline.assembler import SegmentAssembler
from lma_stt.types import WordItem


def word(content, start, end, speaker=None):
    return WordItem(
        content=content, type="pronunciation",
        start_time=start, end_time=end, speaker=speaker,
    )


def transcript(run):
    text = ""
    for item in run["items"]:
        if text and item.type == "pronunciation":
            text += " "
        text += item.content
    return text


def smooth(items):
    asm = SegmentAssembler("call-1")
    return asm._absorb(asm._runs(items))


def test_two_word_noise_run_absorbed_into_previous_strong_run():
    items = [word("alpha", 0.0, 0.3, "spk_0"), word("beta", 0.4, 0.7, "spk_0"),
             word("gamma", 0.8, 1.1, "spk_0"), word("delta", 1.2, 1.5, "spk_0"),
             word("um", 2.0, 2.15, "spk_0"), word("uh", 2.2, 2.35, "spk_0"),
             word("epsilon", 2.6, 2.9, "spk_1"), word("zeta", 3.0, 3.3, "spk_1"),
             word("eta", 3.4, 3.7, "spk_1"), word("theta", 3.8, 4.1, "spk_1"),
             word("iota", 4.2, 4.5, "spk_1"), word("kappa", 4.6, 4.9, "spk_1")]
    runs = smooth(items)
    assert [run["label"] for run in runs] == ["spk_0", "spk_1"]
    assert [run["words"] for run in runs] == [6, 6]
    assert transcript(runs[0]) == "alpha beta gamma delta um uh"
    assert (runs[0]["start"], runs[0]["end"]) == (0.0, 2.35)
    assert (runs[1]["start"], runs[1]["end"]) == (2.6, 4.9)


def test_six_word_turn_stands_alone_after_strong_run():
    items = [word("alpha", 0.0, 0.3, "spk_0"), word("beta", 0.4, 0.7, "spk_0"),
             word("gamma", 0.8, 1.1, "spk_0"), word("delta", 1.2, 1.5, "spk_0"),
             word("echo", 2.0, 2.28, "spk_1"), word("fox", 2.4, 2.68, "spk_1"),
             word("goes", 2.8, 3.08, "spk_1"), word("hill", 3.2, 3.48, "spk_1"),
             word("ink", 3.6, 3.88, "spk_1"), word("june", 4.0, 4.28, "spk_1")]
    runs = smooth(items)
    assert [run["label"] for run in runs] == ["spk_0", "spk_1"]
    assert [run["words"] for run in runs] == [4, 6]
    assert transcript(runs[1]) == "echo fox goes hill ink june"
    assert (runs[1]["start"], runs[1]["end"]) == (2.0, 4.28)


def test_run_under_half_second_absorbed_despite_enough_words():
    items = [word("alpha", 0.0, 0.3, "spk_0"), word("beta", 0.4, 0.7, "spk_0"),
             word("gamma", 0.8, 1.1, "spk_0"), word("delta", 1.2, 1.5, "spk_0"),
             word("epsilon", 1.8, 2.08, "spk_0"),
             word("zeta", 2.6, 2.63, "spk_1"), word("eta", 2.75, 2.78, "spk_1"),
             word("theta", 2.9, 2.93, "spk_1"), word("iota", 2.98, 3.0, "spk_1")]
    runs = smooth(items)
    assert len(runs) == 1
    assert runs[0]["label"] == "spk_0"
    assert runs[0]["words"] == 9
    assert transcript(runs[0]) == "alpha beta gamma delta epsilon zeta eta theta iota"
    assert runs[0]["end"] == 3.0


def test_three_word_turn_of_one_point_two_seconds_kept_separate():
    items = [word("alpha", 0.0, 0.3, "spk_0"), word("beta", 0.4, 0.7, "spk_0"),
             word("gamma", 0.8, 1.1, "spk_0"), word("delta", 1.2, 1.5, "spk_0"),
             word("epsilon", 1.8, 2.08, "spk_0"),
             word("sure", 3.0, 3.25, "spk_1"), word("thing", 3.45, 3.7, "spk_1"),
             word("done", 3.95, 4.2, "spk_1")]
    runs = smooth(items)
    assert [run["label"] for run in runs] == ["spk_0", "spk_1"]
    assert [run["words"] for run in runs] == [5, 3]
    assert runs[1]["end"] - runs[1]["start"] >= 0.5
    assert transcript(runs[1]) == "sure thing done"


def test_leading_weak_run_prepends_to_first_strong_run():
    items = [word("oh", 0.0, 0.15, "spk_1"),
             word("alpha", 0.5, 0.8, "spk_0"), word("beta", 0.9, 1.2, "spk_0"),
             word("gamma", 1.3, 1.6, "spk_0"), word("delta", 1.7, 2.0, "spk_0"),
             word("epsilon", 2.1, 2.4, "spk_0"), word("zeta", 2.5, 2.8, "spk_0")]
    runs = smooth(items)
    assert len(runs) == 1
    assert runs[0]["label"] == "spk_0"
    assert transcript(runs[0]) == "oh alpha beta gamma delta epsilon zeta"
    assert runs[0]["start"] == 0.0


def test_all_weak_input_collapses_under_first_label():
    items = [word("a", 0.0, 0.1, "spk_0"), word("b", 0.5, 0.6, "spk_1"),
             word("c", 1.0, 1.1, "spk_0")]
    runs = smooth(items)
    assert len(runs) == 1
    assert runs[0]["label"] == "spk_0"
    assert transcript(runs[0]) == "a b c"
