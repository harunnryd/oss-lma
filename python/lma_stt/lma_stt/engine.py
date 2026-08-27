import asyncio
import time
from collections.abc import AsyncIterator, Awaitable, Callable
from typing import Protocol

from lma_stt.config import ConfigurationError, ProviderKind, RuntimeConfig
from lma_stt.deepgram import Connect, DeepgramConfig, DeepgramEngine
from lma_stt.types import MeetingContext, Result


class ResultStream(Protocol):
    async def feed(self, pcm: bytes) -> None: ...

    async def close(self) -> None: ...

    def __aiter__(self) -> AsyncIterator[Result]: ...


class SpeechEngine(Protocol):
    async def start(self, ctx: MeetingContext) -> ResultStream: ...


class EngineRegistry:
    def __init__(self, factory: Callable[[MeetingContext], SpeechEngine]):
        self._factory = factory

    @classmethod
    def from_runtime_config(
        cls,
        config: RuntimeConfig,
        *,
        connect: Connect | None = None,
        clock: Callable[[], float] = time.monotonic,
        sleep: Callable[[float], Awaitable[None]] = asyncio.sleep,
    ) -> "EngineRegistry":
        provider = ProviderKind.parse(config.provider)
        if not isinstance(config.api_key, str) or not config.api_key.strip():
            raise ConfigurationError("runtime configuration is missing an API key")
        if not isinstance(config.model, str) or not config.model.strip():
            raise ConfigurationError("runtime configuration is missing a model")
        if config.language is not None and not isinstance(config.language, str):
            raise ConfigurationError("runtime language must be a string or null")
        if provider is not ProviderKind.DEEPGRAM:
            raise ConfigurationError(f"STT provider is not available: {provider}")

        deepgram_config = DeepgramConfig(
            api_key=config.api_key,
            model=config.model,
            language=config.language or "multi",
        )

        def create(_ctx: MeetingContext) -> SpeechEngine:
            return DeepgramEngine(
                deepgram_config,
                connect=connect,
                clock=clock,
                sleep=sleep,
            )

        return cls(create)

    def create(self, ctx: MeetingContext) -> SpeechEngine:
        return self._factory(ctx)
