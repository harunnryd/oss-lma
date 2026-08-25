import sqlite3
from typing import Protocol

from sidecar.storage.writers import dispatch_write


class PersistenceWriter(Protocol):
    def write(self, event: dict) -> None:
        ...


class NullWriter:
    def write(self, event: dict) -> None:
        return None


class SqliteWriter:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    def write(self, event: dict) -> None:
        dispatch_write(self._conn, event)
