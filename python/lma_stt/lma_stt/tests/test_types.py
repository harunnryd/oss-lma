from typing import Literal

from lma_stt.types import (
    MeetingContext,
    ProviderAuthError,
    ProviderResetError,
    Result,
    WordItem,
)


def test_meeting_context_shape():
    ctx: MeetingContext = {
        "call_id": "m-1",
        "sample_rate": 48000,
        "diarize": {"system": True, "mic": True},
        "language_hints": [],
    }
    assert MeetingContext.__annotations__ == {
        "call_id": str,
        "sample_rate": int,
        "diarize": dict,
        "language_hints": list[str],
    }
    assert ctx["call_id"] == "m-1"


def test_word_item_shape():
    item: WordItem = {
        "content": "budget",
        "type": "pronunciation",
        "start_time": 12.42,
        "end_time": 12.88,
        "speaker": "spk_1",
        "channel": "CALLER",
        "result_id": "r-0",
    }
    assert WordItem.__annotations__ == {
        "content": str,
        "type": Literal["pronunciation", "punctuation"],
        "start_time": float,
        "end_time": float,
        "speaker": str | None,
        "channel": Literal["CALLER", "AGENT"],
        "result_id": str,
    }


def test_result_shape():
    result: Result = {"result_id": "r-0", "is_final": False, "items": []}
    assert Result.__annotations__ == {
        "result_id": str,
        "is_final": bool,
        "items": list[WordItem],
    }


def test_provider_errors_are_distinct():
    assert issubclass(ProviderAuthError, Exception)
    assert issubclass(ProviderResetError, Exception)
    assert ProviderAuthError is not ProviderResetError
