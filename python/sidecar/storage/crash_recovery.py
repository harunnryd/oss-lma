import sqlite3


def sweep_stale_partials(conn: sqlite3.Connection) -> list[tuple[str, str]]:
    rows = conn.execute(
        "SELECT meeting_id, segment_id FROM segments "
        "WHERE meeting_id IN (SELECT id FROM meetings WHERE status = 'RECORDING') "
        "AND is_partial = 1"
    ).fetchall()
    if not rows:
        return []
    conn.execute(
        "UPDATE segments SET is_partial = -1 "
        "WHERE meeting_id IN (SELECT id FROM meetings WHERE status = 'RECORDING') "
        "AND is_partial = 1"
    )
    conn.commit()
    return [(row["meeting_id"], row["segment_id"]) for row in rows]


class PendingDeletes:
    def __init__(self) -> None:
        self._items: list[tuple[str, str]] = []

    def push(self, items) -> None:
        self._items.extend(items)

    def consume(self, call_id: str) -> list[tuple[str, str]]:
        matched = [item for item in self._items if item[0] == call_id]
        self._items = [item for item in self._items if item[0] != call_id]
        return matched

    def __len__(self) -> int:
        return len(self._items)
