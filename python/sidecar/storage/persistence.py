import sqlite3
from typing import Protocol

from sidecar.storage.writers import dispatch_write, write_meeting_started


class PersistenceWriter(Protocol):
    def write(self, event: dict) -> None:
        ...

    def write_meeting_started(self, ev: dict, *, return_offset: bool = False) -> int | None:
        ...


class NullWriter:
    def write(self, event: dict) -> None:
        return None

    def write_meeting_started(self, ev: dict, *, return_offset: bool = False) -> int | None:
        return None


class SqliteWriter:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    def write(self, event: dict) -> None:
        dispatch_write(self._conn, event)

    def write_meeting_started(self, ev: dict, *, return_offset: bool = False) -> int | None:
        return write_meeting_started(self._conn, ev, return_offset=return_offset)
