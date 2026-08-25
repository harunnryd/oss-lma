from contracts import load_error_codes


def test_stt_stream_reset_carries_retry_limits():
    limits = load_error_codes()["STT_STREAM_RESET"]["limits"]
    assert limits == {
        "max_consecutive": 5,
        "reset_after_session_seconds": 10,
        "backoff_start_ms": 500,
        "backoff_ceiling_ms": 10000,
    }


def test_vp_manual_action_required_escalates_to_failed():
    entry = load_error_codes()["VP_MANUAL_ACTION_REQUIRED"]
    assert entry["timeout_seconds"] == 300
    assert entry["on_timeout"] == "FAILED"


def test_port_bind_failed_caps_attempts_at_ten():
    assert load_error_codes()["PORT_BIND_FAILED"]["max_attempts"] == 10


def test_unknown_code_lookup_returns_none():
    assert load_error_codes().get("NOT_A_REAL_CODE") is None