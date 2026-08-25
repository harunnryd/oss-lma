from lma_pipeline import SegmentAssembler
from lma_stt.types import Result, WordItem

from sidecar.frames import serialize_event


def test_unlabelled_emission_omits_speaker_and_passes_outbound_schema():
    asm = SegmentAssembler("call-1")
    result = Result(
        result_id="r1",
        channel="CALLER",
        is_partial=True,
        items=[
            WordItem(content="hello", type="pronunciation",
                     start_time=0.0, end_time=0.3, speaker=None),
        ],
    )
    out = asm.on_result(result)
    assert len(out) == 1
    assert "Speaker" not in out[0]
    serialize_event(out[0])
