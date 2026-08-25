import dataclasses

import pytest

from lma_pipeline.assembler import AssemblerConfig, SegmentAssembler
from lma_stt.types import Result


def test_config_defaults():
    config = AssemblerConfig()
    assert config.min_run_words == 3
    assert config.min_run_seconds == 0.5
    assert config.max_segment_seconds == 20.0


def test_config_is_frozen():
    config = AssemblerConfig()
    with pytest.raises(dataclasses.FrozenInstanceError):
        config.min_run_words = 5


def test_empty_result_emits_nothing():
    assembler = SegmentAssembler("call-1")
    out = assembler.on_result(
        Result(result_id="r1", channel="CALLER", is_partial=True, items=[])
    )
    assert out == []


def test_custom_config_is_honored():
    assembler = SegmentAssembler("call-1", AssemblerConfig(min_run_words=7))
    assert assembler.config.min_run_words == 7
