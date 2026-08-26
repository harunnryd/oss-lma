import time
from collections.abc import Callable
from dataclasses import dataclass, field
from pathlib import Path

import yaml


def _now_ms() -> int:
    return int(time.time() * 1000)


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


@dataclass
class ReconnectState:
    consecutive_failures: int = 0
    next_backoff_ms: int = 0
    last_failure_at_ms: int | None = None
    clock: Callable[[], int] = field(default=_now_ms)

    def record_failure(self, now_ms: int) -> None:
        self.consecutive_failures += 1
        self.last_failure_at_ms = now_ms
        backoff = _next_backoff(
            self.consecutive_failures,
            load_reconnect_policy().backoff_start_ms,
            load_reconnect_policy().backoff_ceiling_ms,
        )
        self.next_backoff_ms = backoff

    def record_success(self) -> None:
        self.consecutive_failures = 0
        self.last_failure_at_ms = None
        self.next_backoff_ms = 0

    def maybe_reset_on_idle(self, now_ms: int) -> None:
        policy = load_reconnect_policy()
        if self.last_failure_at_ms is None:
            return
        if (now_ms - self.last_failure_at_ms) >= policy.reset_after_session_seconds * 1000:
            self.record_success()


def _next_backoff(consecutive: int, start_ms: int, ceiling_ms: int) -> int:
    backoff = start_ms
    for _ in range(consecutive - 1):
        backoff *= 2
        if backoff >= ceiling_ms:
            return ceiling_ms
    return min(backoff, ceiling_ms)
