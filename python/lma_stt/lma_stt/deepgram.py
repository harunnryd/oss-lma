import asyncio
import contextlib
import json
import time
from collections.abc import Awaitable, Callable
from dataclasses import dataclass, replace
from typing import Any

import websockets

from lma_stt.types import (
    MeetingContext,
    ProviderAuthError,
    ProviderResetError,
    Result,
    WordItem,
)

_PUNCTUATION_SUFFIXES = (",", ".", "!", "?", ";", ":")

Connect = Callable[[str, dict[str, str]], Awaitable[Any]]


@dataclass
class DeepgramConfig:
    api_key: str
    model: str = "nova-3"
    language: str = "multi"
    sample_rate: int = 48000
    endpointing_ms: int | None = 100


def build_url(cfg: DeepgramConfig) -> str:
    params = [
        ("encoding", "linear16"),
        ("multichannel", "true"),
        ("channels", "2"),
        ("sample_rate", str(cfg.sample_rate)),
        ("model", cfg.model),
        ("language", cfg.language),
        ("interim_results", "true"),
        ("smart_format", "true"),
        ("diarize", "true"),
    ]
    if cfg.endpointing_ms is not None:
        params.append(("endpointing", str(cfg.endpointing_ms)))
    query = "&".join(f"{key}={value}" for key, value in params)
    return f"wss://api.deepgram.com/v1/listen?{query}"


def _word_type(word: str, punctuated_word: str | None) -> str:
    if punctuated_word and punctuated_word != word and punctuated_word.endswith(_PUNCTUATION_SUFFIXES):
        return "punctuation"
    return "pronunciation"


def map_message(message: dict) -> Result:
    metadata = message["metadata"]
    result_id = f"{metadata['request_id']}-{metadata['sequence']}"
    is_final = bool(message["is_final"])
    channel = "CALLER" if message["channel_index"][0] == 0 else "AGENT"
    items: list[WordItem] = []
    for w in message["channel"]["alternatives"][0]["words"]:
        word = w["word"]
        punctuated = w.get("punctuated_word")
        speaker = None
        if is_final and w.get("speaker") is not None:
            speaker = f"spk_{w['speaker']}"
        items.append(
            {
                "content": punctuated if punctuated else word,
                "type": _word_type(word, punctuated),
                "start_time": float(w["start"]),
                "end_time": float(w["end"]),
                "speaker": speaker,
                "channel": channel,
                "result_id": result_id,
            }
        )
    return {"result_id": result_id, "is_final": is_final, "items": items}


async def default_connect(url: str, headers: dict[str, str]) -> Any:
    return await websockets.connect(url, additional_headers=headers)


class DeepgramResultStream:
    def __init__(self, conn: Any, clock: Callable[[], float] = time.monotonic):
        self.conn = conn
        self.clock = clock
        self.last_audio_at = clock()
        self.keepalive_task: asyncio.Task | None = None
        self._closing = False

    async def feed(self, pcm: bytes) -> None:
        self.last_audio_at = self.clock()
        await self.conn.send(pcm)

    async def close(self) -> None:
        if self._closing:
            return
        self._closing = True
        if self.keepalive_task is not None:
            self.keepalive_task.cancel()
            with contextlib.suppress(asyncio.CancelledError):
                await self.keepalive_task
        try:
            await self.conn.send(json.dumps({"type": "CloseStream"}))
        finally:
            await self.conn.close()

    def __aiter__(self) -> "DeepgramResultStream":
        return self

    async def __anext__(self) -> Result:
        while True:
            if self._closing:
                raise StopAsyncIteration
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
            if kind == "Results":
                return map_message(message)
            if kind == "Error":
                raise ProviderResetError(str(message.get("description", "provider error frame")))


class DeepgramEngine:
    def __init__(
        self,
        config: DeepgramConfig,
        connect: Connect | None = None,
        clock: Callable[[], float] = time.monotonic,
        sleep: Callable[[float], Awaitable[None]] = asyncio.sleep,
        interval_s: float = 5.0,
    ):
        self.config = config
        self._connect = connect or default_connect
        self.clock = clock
        self.sleep = sleep
        self.interval_s = interval_s

    async def start(self, ctx: MeetingContext) -> DeepgramResultStream:
        cfg = replace(self.config, sample_rate=ctx["sample_rate"])
        conn = await self._connect(build_url(cfg), {"Authorization": f"Token {cfg.api_key}"})
        status = conn.response.status_code
        if status in (401, 403):
            raise ProviderAuthError(f"handshake rejected with HTTP {status}")
        if status >= 400:
            raise ProviderResetError(f"handshake failed with HTTP {status}")
        stream = DeepgramResultStream(conn, clock=self.clock)
        stream.keepalive_task = asyncio.create_task(self._keepalive_loop(stream))
        return stream

    async def _keepalive_loop(self, stream: DeepgramResultStream) -> None:
        last_sent = self.clock()
        while True:
            await self.sleep(max(0.05, self.interval_s / 10))
            now = self.clock()
            if (
                now - stream.last_audio_at >= self.interval_s
                and now - last_sent >= self.interval_s
            ):
                await stream.conn.send(json.dumps({"type": "KeepAlive"}))
                last_sent = now
