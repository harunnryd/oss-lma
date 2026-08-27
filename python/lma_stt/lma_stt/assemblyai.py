import asyncio
import json
import time
from collections.abc import Awaitable, Callable
from dataclasses import dataclass
from typing import Any

import websockets

from lma_stt.types import MeetingContext, ProviderAuthError, ProviderResetError, Result, WordItem

Connect = Callable[[str, dict[str, str]], Awaitable[Any]]
_PUNCTUATION = (",", ".", "!", "?", ";", ":")


@dataclass(frozen=True)
class AssemblyAIConfig:
    api_key: str
    model: str = "u3-rt-pro"
    language: str | None = None


def build_url(config: AssemblyAIConfig, sample_rate: int) -> str:
    params = [f"sample_rate={sample_rate}", f"speech_model={config.model}", "format_turns=true"]
    if config.language:
        params.append(f"language={config.language}")
    return "wss://streaming.assemblyai.com/v3/ws?" + "&".join(params)


def _result_id(begin_id: str, message: dict[str, Any]) -> str:
    return f"{begin_id}-turn-{message['turn_order']}"


def map_message(message: dict[str, Any], *, begin_id: str, channel: str) -> Result:
    result_id = _result_id(begin_id, message)
    items = []
    for word in message.get("words", []):
        content = word.get("text", word.get("word", ""))
        speaker = word.get("speaker", message.get("speaker_label"))
        items.append(
            WordItem(
                content=content,
                type="punctuation" if content.endswith(_PUNCTUATION) else "pronunciation",
                start_time=float(word.get("start", word.get("start_time", 0))) / 1000,
                end_time=float(word.get("end", word.get("end_time", 0))) / 1000,
                speaker=None if speaker is None else f"spk_{speaker}",
                channel=channel,  # type: ignore[arg-type]
                result_id=result_id,
            )
        )
    return {
        "result_id": result_id,
        "is_partial": not bool(message.get("end_of_turn")),
        "items": items,
    }


def map_messages(messages: list[dict[str, Any]], *, channel: str = "CALLER") -> list[Result]:
    begin_id = "unknown"
    results = []
    for message in messages:
        if message.get("type") == "Begin":
            begin_id = str(message["id"])
        elif message.get("type") in {"Turn", "SpeakerRevision"}:
            results.append(map_message(message, begin_id=begin_id, channel=channel))
    return results


async def default_connect(url: str, headers: dict[str, str]) -> Any:
    return await websockets.connect(url, additional_headers=headers)


class AssemblyAIResultStream:
    def __init__(self, conn: Any, channel: str, clock: Callable[[], float] = time.monotonic):
        self.conn = conn
        self.channel = channel
        self.clock = clock
        self.begin_id = "unknown"
        self._closing = False

    async def feed(self, pcm: bytes) -> None:
        await self.conn.send(pcm)

    async def close(self) -> None:
        if self._closing:
            return
        self._closing = True
        try:
            await self.conn.send(json.dumps({"type": "Terminate"}))
        finally:
            await self.conn.close()

    def __aiter__(self) -> "AssemblyAIResultStream":
        return self

    async def __anext__(self) -> Result:
        while not self._closing:
            try:
                raw = await self.conn.recv()
            except asyncio.CancelledError:
                raise
            except Exception as exc:
                raise ProviderResetError(f"{type(exc).__name__}: {exc}") from exc
            if isinstance(raw, bytes):
                continue
            message = json.loads(raw)
            kind = message.get("type")
            if kind == "Begin":
                self.begin_id = str(message["id"])
            elif kind in {"Turn", "SpeakerRevision"}:
                return map_message(message, begin_id=self.begin_id, channel=self.channel)
            elif kind in {"Error", "Termination"}:
                raise ProviderResetError(
                    str(message.get("error", message.get("message", "provider closed")))
                )
        raise StopAsyncIteration


def deinterleave_s16le(pcm: bytes) -> tuple[bytes, bytes]:
    if len(pcm) % 4:
        raise ValueError("stereo s16le input must contain complete frames")
    return (
        b"".join(pcm[index : index + 2] for index in range(0, len(pcm), 4)),
        b"".join(pcm[index + 2 : index + 4] for index in range(0, len(pcm), 4)),
    )


def _connection_error(exc: Exception) -> Exception:
    response = getattr(exc, "response", None)
    status = getattr(response, "status_code", getattr(exc, "status_code", None))
    message = f"connection failed: {type(exc).__name__}: {exc}"
    if status in (401, 403):
        return ProviderAuthError(message)
    return ProviderResetError(message)


class _ChannelResultStream:
    def __init__(self, streams: list[AssemblyAIResultStream]):
        self.streams = streams
        self._pending: dict[asyncio.Task[Result], AssemblyAIResultStream] = {}
        self._ready: list[Result] = []

    async def feed(self, pcm: bytes) -> None:
        for stream, channel_pcm in zip(self.streams, deinterleave_s16le(pcm), strict=True):
            await stream.feed(channel_pcm)

    async def close(self) -> None:
        await asyncio.gather(*(stream.close() for stream in self.streams))

    def __aiter__(self) -> "_ChannelResultStream":
        return self

    async def __anext__(self) -> Result:
        if self._ready:
            return self._ready.pop(0)
        if not self._pending:
            self._pending = {asyncio.create_task(anext(stream)): stream for stream in self.streams}
        done, _ = await asyncio.wait(self._pending, return_when=asyncio.FIRST_COMPLETED)
        for task in sorted(done, key=lambda item: self.streams.index(self._pending[item])):
            stream = self._pending.pop(task)
            self._ready.append(task.result())
            self._pending[asyncio.create_task(anext(stream))] = stream
        return self._ready.pop(0)


class AssemblyAIEngine:
    def __init__(
        self,
        config: AssemblyAIConfig,
        connect: Connect | None = None,
        clock: Callable[[], float] = time.monotonic,
    ):
        self.config = config
        self._connect = connect or default_connect
        self.clock = clock

    async def start(self, ctx: MeetingContext) -> _ChannelResultStream:
        streams = []
        for channel in ("CALLER", "AGENT"):
            try:
                conn = await self._connect(
                    build_url(self.config, ctx["sample_rate"]),
                    {"Authorization": self.config.api_key},
                )
            except Exception as exc:
                raise _connection_error(exc) from exc
            status = conn.response.status_code
            if status in (401, 403):
                raise ProviderAuthError(f"handshake rejected with HTTP {status}")
            if status >= 400:
                raise ProviderResetError(f"handshake failed with HTTP {status}")
            streams.append(AssemblyAIResultStream(conn, channel, clock=self.clock))
        return _ChannelResultStream(streams)
