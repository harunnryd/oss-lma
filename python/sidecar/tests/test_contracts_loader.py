import pytest

import contracts
from contracts import ContractsError, load_error_codes, load_schema


def test_load_schema_returns_parsed_events_schema():
    schema = load_schema()
    assert schema["title"] == "oss-lma sidecar wire protocol"
    assert len(schema["oneOf"]) == 15


def test_load_error_codes_keys_every_code_in_errors_yaml():
    codes = load_error_codes()
    assert set(codes) == {
        "STT_PROVIDER_AUTH",
        "STT_STREAM_RESET",
        "LINK_DISCONNECTED",
        "CAPTURE_DEVICE_LOST",
        "CAPTURE_PERMISSION_DENIED",
        "VP_CONTAINER_FAILED",
        "VP_MANUAL_ACTION_REQUIRED",
        "AGENT_TOOL_FAILURE",
        "RAG_EMBEDDING_UNAVAILABLE",
        "DB_WRITE_CONFLICT",
        "SIDECAR_UNAVAILABLE",
        "PORT_BIND_FAILED",
    }
    assert codes["STT_STREAM_RESET"]["source"] == "python"


def test_missing_schema_file_raises(monkeypatch, tmp_path):
    monkeypatch.setattr(contracts, "CONTRACTS_ROOT", tmp_path)
    with pytest.raises(ContractsError):
        load_schema()


def test_malformed_schema_json_raises(monkeypatch, tmp_path):
    (tmp_path / "events.schema.json").write_text("{not json", encoding="utf-8")
    monkeypatch.setattr(contracts, "CONTRACTS_ROOT", tmp_path)
    with pytest.raises(ContractsError):
        load_schema()


def test_malformed_errors_yaml_raises(monkeypatch, tmp_path):
    (tmp_path / "errors.yaml").write_text("errors:\n  - code: [unclosed\n", encoding="utf-8")
    monkeypatch.setattr(contracts, "CONTRACTS_ROOT", tmp_path)
    with pytest.raises(ContractsError):
        load_error_codes()
