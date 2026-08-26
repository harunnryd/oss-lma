import time


def _float_seconds_to_ms(value: float) -> int:
    return round(value * 1000)


def _bool_to_int(value: bool) -> int:
    return 1 if value else 0


def _parse_iso_to_ms(value: str | None) -> int | None:
    if value is None:
        return None
    from datetime import datetime

    cleaned = value.replace("Z", "+00:00")
    parsed = datetime.fromisoformat(cleaned)
    return int(parsed.timestamp() * 1000)


def normalize_meeting_started(ev: dict) -> tuple:
    started_at = _parse_iso_to_ms(ev.get("CreatedAt"))
    if started_at is None:
        started_at = int(time.time() * 1000)
    return (ev["CallId"], "LOCAL", started_at)


def normalize_meeting_ended(ev: dict) -> tuple:
    ended_at = _parse_iso_to_ms(ev.get("CreatedAt"))
    if ended_at is None:
        ended_at = int(time.time() * 1000)
    return (ev["CallId"], "COMPLETED", ended_at)


def normalize_segment(ev: dict) -> tuple:
    speaker = ev.get("Speaker")
    if speaker == "":
        speaker = None
    return (
        ev["SegmentId"],
        ev["CallId"],
        ev["Channel"],
        speaker,
        _float_seconds_to_ms(ev["StartTime"]),
        _float_seconds_to_ms(ev["EndTime"]),
        ev["Transcript"],
        ev["Transcript"],
        _bool_to_int(ev["IsPartial"]),
        ev.get("SentimentScore"),
    )


def normalize_summary(ev: dict) -> tuple:
    return (ev["CallId"], ev["Section"], ev["SummaryText"])


def normalize_agent_assist(ev: dict) -> tuple:
    return (
        ev["SegmentId"],
        ev["CallId"],
        ev["Transcript"],
        _bool_to_int(ev["IsPartial"]),
        ev.get("TriggerSegmentId"),
    )


def normalize_agent_token(ev: dict) -> tuple:
    return (ev["QueryId"], ev["CallId"], ev["Seq"], ev["Delta"])


def normalize_thinking_step(ev: dict) -> tuple:
    return (
        ev["QueryId"],
        ev["CallId"],
        ev["Seq"],
        ev["StepType"],
        ev.get("Content"),
    )