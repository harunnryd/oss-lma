from pathlib import Path

import sqlite3

from sidecar.storage.connection import open_db


def test_open_db_returns_connection():
    conn = open_db(Path(":memory:"))
    assert isinstance(conn, sqlite3.Connection)


def test_open_db_sets_wal_journal_mode(tmp_path):
    conn = open_db(tmp_path / "lma.db")
    mode = conn.execute("PRAGMA journal_mode").fetchone()[0]
    assert mode.lower() == "wal"


def test_open_db_enables_foreign_keys(tmp_path):
    conn = open_db(tmp_path / "lma.db")
    fk = conn.execute("PRAGMA foreign_keys").fetchone()[0]
    assert fk == 1


def test_open_db_sets_busy_timeout(tmp_path):
    conn = open_db(tmp_path / "lma.db")
    timeout = conn.execute("PRAGMA busy_timeout").fetchone()[0]
    assert timeout == 5000


def test_open_db_uses_row_factory(tmp_path):
    conn = open_db(tmp_path / "lma.db")
    conn.execute("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)")
    conn.execute("INSERT INTO t (name) VALUES ('x')")
    row = conn.execute("SELECT * FROM t").fetchone()
    assert row["name"] == "x"


def test_open_db_is_idempotent(tmp_path):
    path = tmp_path / "lma.db"
    conn1 = open_db(path)
    conn1.close()
    conn2 = open_db(path)
    mode = conn2.execute("PRAGMA journal_mode").fetchone()[0]
    assert mode.lower() == "wal"
    conn2.close()
