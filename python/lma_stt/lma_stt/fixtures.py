import json
from pathlib import Path

from lma_stt.types import WordItem

FIXTURE_DIR = Path(__file__).resolve().parent.parent / "tests" / "fixtures"


def load_fixture(provider: str, name: str) -> tuple[list[dict], list[dict]]:
    base = FIXTURE_DIR / provider
    messages = [
        json.loads(line)
        for line in (base / f"{name}.ndjson").read_text().splitlines()
        if line.strip()
    ]
    expected = json.loads((base / f"{name}.expected.json").read_text())
    for result in expected:
        result["items"] = [WordItem(**item) for item in result["items"]]
    return messages, expected
