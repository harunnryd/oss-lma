import sqlite3


def sweep_stale_partials(conn: sqlite3.Connection) -> int:
    cursor = conn.execute(
        "UPDATE segments SET is_partial = -1 "
        "WHERE meeting_id IN (SELECT id FROM meetings WHERE status = 'RECORDING') "
        "AND is_partial = 1"
    )
    conn.commit()
    return cursor.rowcount