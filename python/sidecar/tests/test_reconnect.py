from sidecar.reconnect import load_reconnect_policy


def test_load_reconnect_policy_reads_stt_stream_reset_limits():
    policy = load_reconnect_policy()
    assert policy.max_consecutive == 5
    assert policy.reset_after_session_seconds == 10
    assert policy.backoff_start_ms == 500
    assert policy.backoff_ceiling_ms == 10000


def test_reconnect_policy_is_frozen():
    from dataclasses import FrozenInstanceError
    import pytest

    policy = load_reconnect_policy()
    with pytest.raises(FrozenInstanceError):
        policy.max_consecutive = 999
