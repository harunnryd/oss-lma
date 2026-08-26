from dataclasses import dataclass
from pathlib import Path

import yaml


@dataclass(frozen=True)
class ReconnectPolicy:
    max_consecutive: int
    reset_after_session_seconds: int
    backoff_start_ms: int
    backoff_ceiling_ms: int


def load_reconnect_policy() -> ReconnectPolicy:
    errors_path = Path(__file__).resolve().parents[2] / "contracts" / "errors.yaml"
    doc = yaml.safe_load(errors_path.read_text(encoding="utf-8"))
    for entry in doc["errors"]:
        if entry["code"] == "STT_STREAM_RESET":
            limits = entry["limits"]
            return ReconnectPolicy(
                max_consecutive=limits["max_consecutive"],
                reset_after_session_seconds=limits["reset_after_session_seconds"],
                backoff_start_ms=limits["backoff_start_ms"],
                backoff_ceiling_ms=limits["backoff_ceiling_ms"],
            )
    raise ValueError("STT_STREAM_RESET not found in contracts/errors.yaml")
