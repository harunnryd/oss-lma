import re
import sqlite3
import time
from pathlib import Path

_VERSION_PATTERN = re.compile(r"^(\d{3,})_.+\.sql$")


def apply_migrations(conn: sqlite3.Connection, dir: Path) -> list[int]:
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_version ("
        "  id INTEGER PRIMARY KEY,"
        "  version INTEGER NOT NULL UNIQUE,"
        "  applied_at INTEGER NOT NULL"
        ")"
    )
    conn.commit()

    applied_versions = {
        row["version"] for row in conn.execute("SELECT version FROM schema_version").fetchall()
    }

    if not dir.exists():
        return []

    pending: list[tuple[int, Path]] = []
    for path in sorted(dir.iterdir()):
        match = _VERSION_PATTERN.match(path.name)
        if match is None:
            continue
        version = int(match.group(1))
        if version in applied_versions:
            continue
        pending.append((version, path))

    pending.sort(key=lambda item: item[0])

    applied: list[int] = []
    pending_versions = {version for version, _ in pending}
    for version, path in pending:
        if (
            version > 1
            and (version - 1) not in applied_versions
            and (version - 1) not in pending_versions
        ):
            break
        sql = path.read_text(encoding="utf-8")
        try:
            conn.execute("BEGIN")
            conn.executescript(sql)
            conn.execute(
                "INSERT INTO schema_version (version, applied_at) VALUES (?, ?)",
                (version, int(time.time() * 1000)),
            )
            conn.execute("COMMIT")
        except sqlite3.DatabaseError:
            conn.execute("ROLLBACK")
            raise
        applied_versions.add(version)
        applied.append(version)

    return applied
