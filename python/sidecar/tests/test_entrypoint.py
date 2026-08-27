import os
import signal
import subprocess
import sys
import json
from pathlib import Path

import io

from sidecar import __main__ as entrypoint

from tests.helpers import READY_LINE

PYTHON_DIR = Path(__file__).resolve().parents[2]
WORKSPACE_ROOT = PYTHON_DIR.parent


def subprocess_env(**extra: str) -> dict[str, str]:
    package_paths = [
        str(WORKSPACE_ROOT / "python" / "lma_stt"),
        str(WORKSPACE_ROOT / "python" / "lma_pipeline"),
        str(WORKSPACE_ROOT / "python"),
    ]
    existing = os.environ.get("PYTHONPATH")
    if existing:
        package_paths.append(existing)
    return {**os.environ, "PYTHONPATH": os.pathsep.join(package_paths), **extra}


def runtime_payload() -> bytes:
    payload = json.dumps(
        {
            "provider": "deepgram",
            "model": "nova-3",
            "language": "en",
            "azureRegion": None,
            "apiKey": "provider-secret",
        }
    ).encode()
    return len(payload).to_bytes(4, "big") + payload


async def test_main_returns_one_when_binding_fails(monkeypatch):
    async def refuse(*args, **kwargs):
        raise OSError("address in use")

    monkeypatch.setattr("sidecar.server.serve", refuse)
    assert await entrypoint.main(stdin=io.BytesIO(runtime_payload())) == 1


async def test_main_reads_private_runtime_payload_and_uses_real_engine(monkeypatch, tmp_path):
    captured = {}

    async def fake_run_server(engine_factory, **kwargs):
        captured["engine"] = engine_factory(
            {
                "call_id": "m-test",
                "sample_rate": 16000,
                "diarize": {"system": True, "mic": True},
                "language_hints": [],
            }
        )

    monkeypatch.setattr(entrypoint, "run_server", fake_run_server)
    monkeypatch.setenv("LMA_DB_PATH", str(tmp_path / "lma.db"))

    assert await entrypoint.main(stdin=io.BytesIO(runtime_payload())) == 0
    assert captured["engine"].__class__.__name__ == "DeepgramEngine"


def test_subprocess_prints_ready_line_then_shuts_down_on_sigterm():
    proc = subprocess.Popen(
        [sys.executable, "-m", "sidecar"],
        cwd=PYTHON_DIR,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        stdin=subprocess.PIPE,
        env=subprocess_env(),
    )
    try:
        proc.stdin.buffer.write(runtime_payload())
        proc.stdin.buffer.flush()
        proc.stdin.close()
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
        env=subprocess_env(LMA_DB_PATH=str(tmp_path / "lma.db")),
        text=True,
        stdin=subprocess.PIPE,
    )
    try:
        proc.stdin.buffer.write(runtime_payload())
        proc.stdin.buffer.flush()
        proc.stdin.close()
        line = proc.stdout.readline()
        assert READY_LINE.fullmatch(line) is not None
        proc.send_signal(signal.SIGTERM)
        assert proc.wait(timeout=10) == 0
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait(timeout=5)
