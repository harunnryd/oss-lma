import io

import pytest

from lma_stt.config import ConfigurationError, RuntimeConfig, read_runtime_config
from lma_stt.deepgram import DeepgramEngine
from lma_stt.engine import EngineRegistry


CTX = {
    "call_id": "m-test",
    "sample_rate": 16000,
    "diarize": {"system": True, "mic": True},
    "language_hints": [],
}


def test_registry_selects_deepgram_from_runtime_config():
    registry = EngineRegistry.from_runtime_config(
        RuntimeConfig(
            provider="deepgram",
            model="nova-3",
            language="en",
            azure_region=None,
            api_key="provider-secret",
        )
    )

    engine = registry.create(CTX)

    assert isinstance(engine, DeepgramEngine)
    assert engine.config.api_key == "provider-secret"
    assert engine.config.model == "nova-3"
    assert engine.config.language == "en"


@pytest.mark.parametrize(
    "config",
    [
        RuntimeConfig("unknown", "nova-3", None, None, "provider-secret"),
        RuntimeConfig("deepgram", "nova-3", None, None, ""),
    ],
)
def test_unknown_provider_and_missing_secret_are_rejected(config):
    with pytest.raises(ConfigurationError):
        EngineRegistry.from_runtime_config(config)


def test_non_utf8_runtime_payload_is_rejected_as_configuration_error():
    payload = b"\xff"

    with pytest.raises(ConfigurationError, match="valid JSON"):
        read_runtime_config(io.BytesIO(len(payload).to_bytes(4, "big") + payload))
