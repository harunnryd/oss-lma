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
        state = self._state(result["channel"])
        if not result["items"]:
            return []
        if state["result_id"] != result["result_id"]:
            state["result_id"] = result["result_id"]
            state["origin"] = None
            state["settled_emitted"] = 0
        return self._emit_diarized(result, state)

    def _select(self, windows: list[dict], is_partial: bool,
                settled_emitted: int) -> tuple[list[dict], int]:
        if not is_partial:
            return windows, settled_emitted
        emit = [w for w in windows if w["index"] >= settled_emitted]
        if not emit:
            return emit, settled_emitted
        highest = emit[-1]["index"]
        below = sum(1 for w in emit if w["index"] < highest)
        return emit, settled_emitted + below

    def _build_windows(self, items: list[WordItem], is_final: bool,
                       origin: float) -> list[dict]:
        buckets = self._buckets(items, origin)
        highest = buckets[-1][0]
        return [{"index": index,
                 "runs": self._absorb(self._runs(bucket)),
                 "settled": is_final or index < highest}
                for index, bucket in buckets]

    def _transcript(self, items: list[WordItem]) -> str:
        text = ""
        for item in items:
            if text and item.type == "pronunciation":
                text += " "
            text += item.content
        return text

    def _emit_diarized(self, result: Result, state: dict) -> list[dict]:
        if state["origin"] is None:
            state["origin"] = self._origin(result["items"])
        windows = self._build_windows(result["items"], not result["is_partial"],
                                      state["origin"])
        if result["is_partial"] and state.get("first_partial", True):
            emit = [windows[0]] if windows else []
            state["first_partial"] = False
            state["settled_emitted"] = state["settled_emitted"]
        else:
            emit, state["settled_emitted"] = self._select(
                windows, result["is_partial"], state["settled_emitted"])
            if not result["is_partial"]:
                state["first_partial"] = False
        out: list[dict] = []
        if emit:
            highest_emit = max(w["index"] for w in emit)
            for window in emit:
                for position, run in enumerate(window["runs"]):
                    if result["is_partial"]:
                        partial = window["index"] == highest_emit
                    else:
                        partial = False
                    out.append({
                        "EventType": "ADD_TRANSCRIPT_SEGMENT",
                        "CallId": self.call_id,
                        "SegmentId": (f"{result['result_id']}-{result['channel']}"
                                      f"-w{window['index']}-r{position}"),
                        "Channel": result["channel"],
                        "Speaker": run["label"],
                        "StartTime": run["start"],
                        "EndTime": run["end"],
                        "Transcript": self._transcript(run["items"]),
                        "IsPartial": partial,
                    })
        return out

    def _refresh(self, run: dict) -> None:
        timed = [i for i in run["items"] if i.type == "pronunciation"]
        source = timed if timed else run["items"]
        run["start"] = min(i.start_time for i in source)
        run["end"] = max(i.end_time for i in source)
        run["words"] = sum(1 for i in run["items"] if i.speaker is not None)

    def _runs(self, items: list[WordItem]) -> list[dict]:
        runs: list[dict] = []
        for item in items:
            label = item.speaker
            if runs:
                current = runs[-1]
                if label is None or current["label"] == label:
                    current["items"].append(item)
                    continue
                if current["label"] is None:
                    current["label"] = label
                    current["items"].append(item)
                    continue
            runs.append({"label": label, "items": [item], "words": 0,
                         "start": 0.0, "end": 0.0})
        for run in runs:
            self._refresh(run)
        return runs

    def _weak(self, run: dict) -> bool:
        duration = run["end"] - run["start"]
        return (run["words"] < self.config.min_run_words
                or duration < self.config.min_run_seconds)

    def _absorb(self, runs: list[dict]) -> list[dict]:
        if len(runs) <= 1:
            return runs
        kept: list[dict] = []
        pending: dict | None = None
        for run in runs:
            if self._weak(run):
                if kept:
                    kept[-1]["items"].extend(run["items"])
                elif pending is not None:
                    pending["items"].extend(run["items"])
                else:
                    pending = run
                continue
            if pending is not None:
                run["items"] = pending["items"] + run["items"]
                pending = None
            kept.append(run)
        if pending is not None:
            kept.append(pending)
        coalesced: list[dict] = []
        for run in kept:
            self._refresh(run)
            if coalesced and coalesced[-1]["label"] == run["label"]:
                coalesced[-1]["items"].extend(run["items"])
                self._refresh(coalesced[-1])
                continue
            coalesced.append(run)
        return coalesced
