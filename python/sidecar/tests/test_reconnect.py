from sidecar.reconnect import ReconnectState, load_reconnect_policy


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


def test_initial_state_has_zero_consecutive_failures():
    state = ReconnectState()
    assert state.consecutive_failures == 0
    assert state.next_backoff_ms == 0
    assert state.last_failure_at_ms is None


def test_record_failure_increments_and_schedules_first_backoff():
    state = ReconnectPolicy = __import__("sidecar.reconnect", fromlist=["ReconnectPolicy"]).ReconnectPolicy
    policy = __import__("sidecar.reconnect", fromlist=["ReconnectPolicy"]).ReconnectPolicy(
        max_consecutive=5, reset_after_session_seconds=10,
        backoff_start_ms=500, backoff_ceiling_ms=10000,
    )
    state = ReconnectState()
    state.record_failure(now_ms=1000)
    assert state.consecutive_failures == 1
    assert state.last_failure_at_ms == 1000
    assert state.next_backoff_ms == 500


def test_record_failure_doubles_backoff_until_ceiling():
    policy = __import__("sidecar.reconnect", fromlist=["ReconnectPolicy"]).ReconnectPolicy(
        max_consecutive=5, reset_after_session_seconds=10,
        backoff_start_ms=500, backoff_ceiling_ms=10000,
    )
    state = ReconnectState()
    for i, expected in enumerate([500, 1000, 2000, 4000, 8000], start=1):
        state.record_failure(now_ms=1000 * i)
        assert state.consecutive_failures == i
        assert state.next_backoff_ms == expected
    state.record_failure(now_ms=6000)
    assert state.consecutive_failures == 6
    assert state.next_backoff_ms == 10000


def test_record_success_resets_counter():
    state = ReconnectState()
    state.record_failure(now_ms=1000)
    state.record_failure(now_ms=2000)
    state.record_success()
    assert state.consecutive_failures == 0
    assert state.last_failure_at_ms is None
    assert state.next_backoff_ms == 0


def test_maybe_reset_on_idle_resets_after_threshold():
    state = ReconnectState()
    state.record_failure(now_ms=1000)
    state.maybe_reset_on_idle(now_ms=5000)
    assert state.consecutive_failures == 1
    state.maybe_reset_on_idle(now_ms=11001)
    assert state.consecutive_failures == 0
