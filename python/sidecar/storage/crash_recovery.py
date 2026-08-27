import sqlite3


def sweep_stale_partials(conn: sqlite3.Connection) -> list[tuple[str, str]]:
    """Mark stale partial segments as deleted and return the evicted pairs.

    Returns a list of (call_id, segment_id) tuples for every row whose
    is_partial transitioned from 1 to -1. The list is empty when there
    are no in-progress meetings to clean up.
    """
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
    """Buffer of (call_id, segment_id) pairs awaiting DELETE event emission.

    Reconciliation policy (see `docs/superpowers/specs/2026-08-27-production-core-integration-design.md`):
    - Sessions consume the pairs whose call_id matches their own and emit
      a DELETE_TRANSCRIPT_SEGMENT frame for each one, in addition to
      persisting the row deletion.
    - Pairs for other call_ids remain buffered until a session for that
      call_id opens or the sidecar exits.
    - Process exit drops anything still pending. The database already
      marks those rows with is_partial = -1; the UI does NOT see this
      flag directly, so a row whose DELETE event never fires will remain
      in the webview DOM as a "ghost" partial until either (a) the user
      reconnects to the meeting (the new session drains the buffer) or
      (b) the webview itself is reloaded and re-renders from scratch.

    This narrow window is acceptable because the supervisor respawns the
    sidecar as part of the same health event, and the link reconnect path
    always opens a new session for the live meeting before frames resume.
    """

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
