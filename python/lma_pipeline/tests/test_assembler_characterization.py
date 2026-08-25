from lma_pipeline.assembler import SegmentAssembler
from lma_stt.types import Result, WordItem


TURN_A = ["can", "you", "send", "me", "the", "updated", "budget", "report",
          "before", "our", "standup", "tomorrow", "morning", "please"]
TURN_B = ["sure", "i", "will", "export", "the", "sheet", "tonight", "and",
          "share", "it", "in", "the", "channel", "right", "after", "dinner",
          "with", "the", "whole", "team"]


def turn_a(labelled):
    speaker = "spk_0" if labelled else None
    items = [WordItem(content=w, type="pronunciation", start_time=float(i),
                      end_time=float(i) + 0.5, speaker=speaker)
             for i, w in enumerate(TURN_A)]
    items.append(WordItem(content=".", type="punctuation",
                          start_time=13.5, end_time=13.5, speaker=None))
    return items


def turn_b(first, last, labelled):
    speaker = "spk_1" if labelled else None
    return [WordItem(content=TURN_B[i], type="pronunciation",
                     start_time=15.5 + i, end_time=15.5 + i + 0.5,
                     speaker=speaker)
            for i in range(first, last + 1)]


def period_b():
    return WordItem(content=".", type="punctuation",
                    start_time=35.0, end_time=35.0, speaker=None)


def event(segment_id, speaker, start, end, text, partial):
    e = {
        "EventType": "ADD_TRANSCRIPT_SEGMENT",
        "CallId": "call-35",
        "SegmentId": segment_id,
        "Channel": "CALLER",
        "StartTime": start,
        "EndTime": end,
        "Transcript": text,
        "IsPartial": partial,
    }
    if speaker is not None:
        e["Speaker"] = speaker
    return e


def test_thirty_five_second_two_turn_conversation_characterization():
    asm = SegmentAssembler("call-35")

    p1 = asm.on_result(Result(result_id="rA", channel="CALLER",
                              is_partial=True, items=turn_a(False)[:7]))
    assert p1 == [event("rA-CALLER-w0-r0", None, 0.0, 6.5,
                        "can you send me the updated budget", True)]

    p2 = asm.on_result(Result(
        result_id="rA", channel="CALLER", is_partial=True,
        items=turn_a(False) + turn_b(0, 7, False),
    ))
    assert p2 == [
        event("rA-CALLER-w0-r0", None, 0.0, 20.0,
              "can you send me the updated budget report before our standup "
              "tomorrow morning please. sure i will export the", False),
        event("rA-CALLER-w1-r0", None, 20.5, 23.0, "sheet tonight and", True),
    ]

    p3 = asm.on_result(Result(
        result_id="rA", channel="CALLER", is_partial=True,
        items=turn_a(False) + turn_b(0, 17, False),
    ))
    assert p3 == [event(
        "rA-CALLER-w1-r0", None, 20.5, 33.0,
        "sheet tonight and share it in the channel right after dinner with "
        "the", True,
    )]

    final = asm.on_result(Result(
        result_id="rA", channel="CALLER", is_partial=False,
        items=turn_a(True) + turn_b(0, 19, True) + [period_b()],
    ))
    assert final == [
        event("rA-CALLER-w0-r0", "spk_0", 0.0, 20.0,
              "can you send me the updated budget report before our standup "
              "tomorrow morning please. sure i will export the", False),
        event("rA-CALLER-w1-r0", "spk_1", 20.5, 35.0,
              "sheet tonight and share it in the channel right after dinner "
              "with the whole team.", False),
    ]

    emissions = [*p1, *p2, *p3, *final]
    for emitted in emissions:
        assert emitted["EndTime"] - emitted["StartTime"] <= 20.0
    assert len(emissions) == 6
