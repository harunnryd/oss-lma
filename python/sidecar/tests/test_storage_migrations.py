import sqlite3
from pathlib import Path

from sidecar.storage.connection import open_db
from sidecar.storage.migrations import apply_migrations


def _write_migration(dir: Path, version: int, body: str) -> None:
    (dir / f"{version:03d}_test.sql").write_text(body, encoding="utf-8")


def test_apply_migrations_creates_schema_version_table(tmp_path):
    conn = open_db(tmp_path / "lma.db")
    apply_migrations(conn, tmp_path / "migrations")
    rows = conn.execute(
        "SELECT name FROM sqlite_master WHERE type='table' AND name='schema_version'"
    ).fetchall()
    assert len(rows) == 1


def test_apply_migrations_runs_pending_files(tmp_path):
    migrations_dir = tmp_path / "migrations"
    migrations_dir.mkdir()
    _write_migration(
        migrations_dir,
        1,
        "CREATE TABLE widgets (id INTEGER PRIMARY KEY, name TEXT NOT NULL);",
    )
    conn = open_db(tmp_path / "lma.db")
    applied = apply_migrations(conn, migrations_dir)
    assert applied == [1]
    cols = conn.execute("PRAGMA table_info(widgets)").fetchall()
    assert any(c["name"] == "name" for c in cols)


def test_apply_migrations_runs_multiple_in_order(tmp_path):
    migrations_dir = tmp_path / "migrations"
    migrations_dir.mkdir()
    _write_migration(
        migrations_dir,
        2,
        "CREATE TABLE widgets (id INTEGER PRIMARY KEY);",
    )
    _write_migration(
        migrations_dir,
        1,
        "CREATE TABLE sparks (id INTEGER PRIMARY KEY);",
    )
    conn = open_db(tmp_path / "lma.db")
    applied = apply_migrations(conn, migrations_dir)
    assert applied == [1, 2]


def test_apply_migrations_is_idempotent(tmp_path):
    migrations_dir = tmp_path / "migrations"
    migrations_dir.mkdir()
    _write_migration(
        migrations_dir,
        1,
        "CREATE TABLE widgets (id INTEGER PRIMARY KEY);",
    )
    conn = open_db(tmp_path / "lma.db")
    first = apply_migrations(conn, migrations_dir)
    second = apply_migrations(conn, migrations_dir)
    assert first == [1]
    assert second == []


def test_apply_migrations_records_version_and_timestamp(tmp_path):
    migrations_dir = tmp_path / "migrations"
    migrations_dir.mkdir()
    _write_migration(
        migrations_dir,
        1,
        "CREATE TABLE widgets (id INTEGER PRIMARY KEY);",
    )
    conn = open_db(tmp_path / "lma.db")
    apply_migrations(conn, migrations_dir)
    row = conn.execute("SELECT version, applied_at FROM schema_version").fetchone()
    assert row["version"] == 1
    assert row["applied_at"] > 0


def test_apply_migrations_rolls_back_on_broken_sql(tmp_path):
    migrations_dir = tmp_path / "migrations"
    migrations_dir.mkdir()
    _write_migration(migrations_dir, 1, "CREATE TABLE widgets (id INTEGER PRIMARY KEY);")
    _write_migration(migrations_dir, 2, "THIS IS NOT VALID SQL;")
    conn = open_db(tmp_path / "lma.db")

    import sqlite3 as _sqlite3

    with _sqlite3.connect(":memory:") as dummy:
        dummy.execute("CREATE TABLE x (id INTEGER PRIMARY KEY)")

    try:
        apply_migrations(conn, migrations_dir)
    except sqlite3.DatabaseError:
        pass
    else:
        raise AssertionError("expected DatabaseError on broken SQL")

    rows = conn.execute("SELECT version FROM schema_version").fetchall()
    assert [r["version"] for r in rows] == [1]


def test_apply_migrations_skips_out_of_order_higher_versions(tmp_path):
    migrations_dir = tmp_path / "migrations"
    migrations_dir.mkdir()
    _write_migration(migrations_dir, 5, "CREATE TABLE late (id INTEGER PRIMARY KEY);")
    conn = open_db(tmp_path / "lma.db")
    applied = apply_migrations(conn, migrations_dir)
    assert applied == []
    cols = conn.execute("PRAGMA table_info(late)").fetchall()
    assert cols == []
