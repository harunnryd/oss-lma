import json
from dataclasses import dataclass
from enum import StrEnum
from typing import BinaryIO, Self


class ConfigurationError(ValueError):
    pass


class ProviderKind(StrEnum):
    DEEPGRAM = "deepgram"
    ASSEMBLY_AI = "assemblyAi"
    AZURE = "azure"

    @classmethod
    def parse(cls, value: object) -> Self:
        if not isinstance(value, str):
            raise ConfigurationError("runtime provider must be a string")
        try:
            return cls(value)
        except ValueError as exc:
            raise ConfigurationError(f"unknown STT provider: {value}") from exc


@dataclass(frozen=True)
class RuntimeConfig:
    provider: ProviderKind | str
    model: str
    language: str | None
    azure_region: str | None
    api_key: str

    @classmethod
    def from_payload(cls, payload: object) -> Self:
        if not isinstance(payload, dict):
            raise ConfigurationError("runtime configuration must be an object")
        try:
            return cls(
                provider=payload["provider"],
                model=payload["model"],
                language=payload["language"],
                azure_region=payload["azureRegion"],
                api_key=payload["apiKey"],
            )
        except KeyError as exc:
            raise ConfigurationError(f"runtime configuration is missing {exc.args[0]}") from exc


def read_runtime_config(stdin: BinaryIO) -> RuntimeConfig:
    length_bytes = _read_exact(stdin, 4)
    length = int.from_bytes(length_bytes, "big")
    if length == 0 or length > 1_048_576:
        raise ConfigurationError("runtime configuration has an invalid length")
    try:
        payload = json.loads(_read_exact(stdin, length))
    except json.JSONDecodeError as exc:
        raise ConfigurationError("runtime configuration is not valid JSON") from exc
    return RuntimeConfig.from_payload(payload)


def _read_exact(stdin: BinaryIO, size: int) -> bytes:
    value = stdin.read(size)
    if len(value) != size:
        raise ConfigurationError("runtime configuration ended unexpectedly")
    return value
