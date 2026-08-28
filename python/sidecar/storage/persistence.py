import sqlite3
from typing import Protocol

from sidecar.storage.writers import (
    dispatch_write,
    read_max_segment_end_ms,
    write_meeting_failed,
    write_meeting_started,
    write_meeting_started_update_offset,
)


class PersistenceWriter(Protocol):
    def write(self, event: dict, *, time_offset_ms: int = 0) -> None: ...

    def write_meeting_started(self, ev: dict, *, return_offset: bool = False) -> int | None: ...

    def write_meeting_started_update_offset(
        self, ev: dict, *, time_offset_ms: int, reconnect_attempts: int | None = None
    ) -> None: ...

    def write_meeting_failed(self, ev: dict) -> None: ...

    def read_max_segment_end_ms(self, call_id: str) -> int | None: ...


class NullWriter:
    def write(self, event: dict, *, time_offset_ms: int = 0) -> None:
        return None

    def write_meeting_started(self, ev: dict, *, return_offset: bool = False) -> int | None:
        return None

    def write_meeting_started_update_offset(
        self, ev: dict, *, time_offset_ms: int, reconnect_attempts: int | None = None
    ) -> None:
        return None

    def write_meeting_failed(self, ev: dict) -> None:
        return None

    def read_max_segment_end_ms(self, call_id: str) -> int | None:
        return None


class SqliteWriter:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    def write(self, event: dict, *, time_offset_ms: int = 0) -> None:
        dispatch_write(self._conn, event, time_offset_ms=time_offset_ms)

    def write_meeting_started(self, ev: dict, *, return_offset: bool = False) -> int | None:
        return write_meeting_started(self._conn, ev, return_offset=return_offset)

    def write_meeting_started_update_offset(
        self, ev: dict, *, time_offset_ms: int, reconnect_attempts: int | None = None
    ) -> None:
        return write_meeting_started_update_offset(
            self._conn, ev, time_offset_ms=time_offset_ms, reconnect_attempts=reconnect_attempts
        )

    def write_meeting_failed(self, ev: dict) -> None:
        return write_meeting_failed(self._conn, ev)

    def read_max_segment_end_ms(self, call_id: str) -> int | None:
        return read_max_segment_end_ms(self._conn, call_id)
