import json
import sys
from pathlib import Path

import yaml

CONTRACTS_ROOT = (
    Path(sys._MEIPASS) / "contracts"
    if getattr(sys, "frozen", False)
    else Path(__file__).resolve().parents[2] / "contracts"
)


class ContractsError(RuntimeError):
    pass


def _load(path: Path) -> dict:
    try:
        raw = path.read_text(encoding="utf-8")
    except OSError as exc:
        raise ContractsError(f"unreadable contract file: {path}") from exc
    try:
        parsed = yaml.safe_load(raw) if path.suffix in {".yaml", ".yml"} else json.loads(raw)
    except (yaml.YAMLError, json.JSONDecodeError) as exc:
        raise ContractsError(f"malformed contract file: {path}") from exc
    if not isinstance(parsed, dict):
        raise ContractsError(f"contract file is not a mapping: {path}")
    return parsed


def load_schema() -> dict:
    schema = _load(CONTRACTS_ROOT / "events.schema.json")
    if "$defs" not in schema or "oneOf" not in schema:
        raise ContractsError("events schema missing $defs or oneOf")
    return schema


def load_error_codes() -> dict[str, dict]:
    doc = _load(CONTRACTS_ROOT / "errors.yaml")
    entries = doc.get("errors")
    if not isinstance(entries, list) or not entries:
        raise ContractsError("errors catalog has no entries")
    catalog: dict[str, dict] = {}
    for entry in entries:
        if not isinstance(entry, dict) or "code" not in entry:
            raise ContractsError(f"errors catalog entry missing code: {entry!r}")
        catalog[entry["code"]] = entry
    return catalog
