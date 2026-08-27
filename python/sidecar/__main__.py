import asyncio
import os
import signal
import sys
from collections.abc import Callable
from pathlib import Path
from typing import BinaryIO

from lma_stt.config import ConfigurationError, read_runtime_config
from lma_stt.engine import EngineRegistry, SpeechEngine
from lma_stt.types import MeetingContext

from sidecar.server import BindFailed, run_server
from sidecar.storage.connection import open_db
from sidecar.storage.crash_recovery import sweep_stale_partials
from sidecar.storage.migrations import apply_migrations
from sidecar.storage.persistence import SqliteWriter


def default_engine_factory(registry: EngineRegistry) -> Callable[[MeetingContext], SpeechEngine]:
    return registry.create


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


async def main(stdin: BinaryIO | None = None) -> int:
    try:
        runtime_config = read_runtime_config(stdin if stdin is not None else sys.stdin.buffer)
        registry = EngineRegistry.from_runtime_config(runtime_config)
    except ConfigurationError as exc:
        print(f"invalid sidecar runtime configuration: {exc}", file=sys.stderr, flush=True)
        return 1

    stop = asyncio.Event()
    loop = asyncio.get_running_loop()
    for sig in (signal.SIGINT, signal.SIGTERM):
        loop.add_signal_handler(sig, stop.set)

    db_path = _default_db_path()
    db_path.parent.mkdir(parents=True, exist_ok=True)
    conn = open_db(db_path)
    apply_migrations(conn, Path(__file__).resolve().parent / "storage" / "migrations")
    marked = sweep_stale_partials(conn)
    if marked > 0:
        print(f"recovered {marked} stale partial(s) from previous run", file=sys.stderr, flush=True)
    writer = SqliteWriter(conn)
    record_enabled = _record_meeting_enabled()

    try:
        await run_server(
            default_engine_factory(registry),
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
