import asyncio
import os
import signal
import sys
from pathlib import Path

from lma_stt.fake import FakeEngine

from sidecar.server import BindFailed, run_server
from sidecar.storage.connection import open_db
from sidecar.storage.migrations import apply_migrations
from sidecar.storage.persistence import SqliteWriter


def default_engine_factory(ctx):
    return FakeEngine(script=[])


def _default_db_path() -> Path:
    explicit = os.environ.get("LMA_DB_PATH")
    if explicit:
        return Path(explicit)
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Application Support" / "oss-lma" / "lma.db"
    if sys.platform.startswith("win"):
        appdata = os.environ.get("APPDATA")
        if appdata:
            return Path(appdata) / "oss-lma" / "lma.db"
    xdg = os.environ.get("XDG_DATA_HOME")
    base = Path(xdg) if xdg else Path.home() / ".local" / "share"
    return base / "oss-lma" / "lma.db"


def _record_meeting_enabled() -> bool:
    return os.environ.get("LMA_RECORD_MEETING") == "1"


async def main() -> int:
    stop = asyncio.Event()
    loop = asyncio.get_running_loop()
    for sig in (signal.SIGINT, signal.SIGTERM):
        loop.add_signal_handler(sig, stop.set)

    db_path = _default_db_path()
    db_path.parent.mkdir(parents=True, exist_ok=True)
    conn = open_db(db_path)
    apply_migrations(conn, Path(__file__).resolve().parent / "storage" / "migrations")
    writer = SqliteWriter(conn)
    record_enabled = _record_meeting_enabled()

    try:
        await run_server(
            default_engine_factory,
            stop=stop,
            db_writer=writer,
            record_meeting=record_enabled,
        )
    except BindFailed:
        return 1
    finally:
        conn.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(asyncio.run(main()))