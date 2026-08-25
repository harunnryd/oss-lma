from dataclasses import dataclass

from lma_stt.types import Result, WordItem


@dataclass(frozen=True)
class AssemblerConfig:
    min_run_words: int = 3
    min_run_seconds: float = 0.5
    max_segment_seconds: float = 20.0


class SegmentAssembler:
    def __init__(self, call_id: str, config: AssemblerConfig | None = None):
        self.call_id = call_id
        self.config = config if config is not None else AssemblerConfig()
        self._channels: dict[str, dict] = {}

    def _state(self, channel: str) -> dict:
        if channel not in self._channels:
            self._channels[channel] = {
                "result_id": None,
                "origin": None,
                "settled_emitted": 0,
                "diarized": False,
                "pending_speaker": None,
                "timeline": [],
                "open": {},
            }
        return self._channels[channel]

    def set_active_speaker(self, channel: str, name: str) -> None:
        self._state(channel)["pending_speaker"] = name

    def _origin(self, items: list[WordItem]) -> float:
        starts = [i.start_time for i in items if i.type == "pronunciation"]
        if not starts:
            starts = [i.start_time for i in items]
        return min(starts)

    def _buckets(self, items: list[WordItem], origin: float) -> list[tuple[int, list[WordItem]]]:
        limit = self.config.max_segment_seconds
        grouped: dict[int, list[WordItem]] = {}
        if limit <= 0:
            grouped[0] = list(items)
        else:
            last = 0
            for item in items:
                if item.type == "pronunciation":
                    index = max(0, int((item.start_time - origin) // limit))
                else:
                    index = last
                last = index
                grouped.setdefault(index, []).append(item)
        return [(index, grouped[index]) for index in sorted(grouped)]

    def on_result(self, result: Result) -> list[dict]:
        return []
