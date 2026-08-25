from typing import Literal, TypedDict


class MeetingContext(TypedDict):
    call_id: str
    sample_rate: int
    diarize: dict
    language_hints: list[str]


class WordItem(TypedDict):
    content: str
    type: Literal["pronunciation", "punctuation"]
    start_time: float
    end_time: float
    speaker: str | None
    channel: Literal["CALLER", "AGENT"]
    result_id: str


class Result(TypedDict):
    result_id: str
    is_final: bool
    items: list[WordItem]


class ProviderAuthError(Exception):
    pass


class ProviderResetError(Exception):
    pass
