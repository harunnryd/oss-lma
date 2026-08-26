import os
import signal
import subprocess
import sys
from pathlib import Path

from sidecar import __main__ as entrypoint

from tests.helpers import READY_LINE

PYTHON_DIR = Path(__file__).resolve().parents[2]


async def test_main_returns_one_when_binding_fails(monkeypatch):
    async def refuse(*args, **kwargs):
        raise OSError("address in use")

    monkeypatch.setattr("sidecar.server.serve", refuse)
    assert await entrypoint.main() == 1


def test_subprocess_prints_ready_line_then_shuts_down_on_sigterm():
    proc = subprocess.Popen(
        [sys.executable, "-m", "sidecar"],
        cwd=PYTHON_DIR,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        line = proc.stdout.readline()
        match = READY_LINE.fullmatch(line)
        assert match is not None
        proc.send_signal(signal.SIGTERM)
        assert proc.wait(timeout=10) == 0
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait(timeout=5)


def test_entrypoint_runs_stale_partial_sweep_on_startup(tmp_path, monkeypatch):
    monkeypatch.setenv("LMA_DB_PATH", str(tmp_path / "lma.db"))
    proc = subprocess.Popen(
        [sys.executable, "-m", "sidecar"],
        cwd=PYTHON_DIR,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env={**os.environ, "LMA_DB_PATH": str(tmp_path / "lma.db")},
        text=True,
    )
    try:
        line = proc.stdout.readline()
        assert READY_LINE.fullmatch(line) is not None
        proc.send_signal(signal.SIGTERM)
        assert proc.wait(timeout=10) == 0
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait(timeout=5)
