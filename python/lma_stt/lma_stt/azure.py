import asyncio
import json
import time
import uuid
import wave
from collections.abc import Awaitable, Callable
from dataclasses import dataclass
from io import BytesIO
from typing import Any

import websockets

from lma_stt.types import MeetingContext, ProviderAuthError, ProviderResetError, Result, WordItem

Connect = Callable[[str, dict[str, str]], Awaitable[Any]]
_TICKS_PER_SECOND = 10_000_000
_PUNCTUATION = ",.!?;:"


@dataclass(frozen=True)
class AzureConfig:
    api_key: str
    region: str
    language: str = "en-US"
    model: str = ""


def build_url(config: AzureConfig, connection_id: str) -> str:
    return (
        f"wss://{config.region}.stt.speech.microsoft.com/speech/recognition/conversation/cognitiveservices/v1"
        f"?language={config.language}&format=detailed&X-ConnectionId={connection_id}"
    )


def _attach_punctuation(words: list[dict[str, Any]], display: str) -> list[str]:
    rendered = [str(word["Word"]) for word in words]
    cursor = 0
    for index, word in enumerate(rendered):
        found = display.lower().find(word.lower(), cursor)
        if found < 0:
            continue
        rendered[index] = display[found : found + len(word)]
        cursor = found + len(word)
        while cursor < len(display) and display[cursor] in _PUNCTUATION:
            rendered[index] += display[cursor]
            cursor += 1
    return rendered


def map_message(message: dict[str, Any], *, channel: str) -> Result:
    offset = int(message.get("Offset", 0))
    duration = int(message.get("Duration", 0))
    result_id = str(message.get("Id", f"{offset}-{duration}"))
    best = message.get("NBest", [{}])[0]
    words = best.get("Words", [])
    rendered = _attach_punctuation(words, str(best.get("Display", message.get("DisplayText", ""))))
    items = [
        WordItem(
            content=content,
            type="punctuation" if content.endswith(tuple(_PUNCTUATION)) else "pronunciation",
            start_time=int(word["Offset"]) / _TICKS_PER_SECOND,
            end_time=(int(word["Offset"]) + int(word["Duration"])) / _TICKS_PER_SECOND,
            speaker=None,
            channel=channel,  # type: ignore[arg-type]
            result_id=result_id,
        )
        for word, content in zip(words, rendered, strict=True)
    ]
    return {"result_id": result_id, "is_partial": False, "items": items}


def map_messages(messages: list[dict[str, Any]], *, channel: str = "CALLER") -> list[Result]:
    return [
        map_message(message, channel=channel)
        for message in messages
        if message.get("RecognitionStatus") == "Success"
    ]


def downsample_mono_s16le(pcm: bytes, input_rate: int) -> bytes:
    if input_rate == 16_000:
        return pcm
    samples = memoryview(pcm).cast("h")
    count = len(samples) * 16_000 // input_rate
    return b"".join(
        int(samples[index * input_rate // 16_000]).to_bytes(2, "little", signed=True)
        for index in range(count)
    )


def deinterleave_s16le(pcm: bytes) -> tuple[bytes, bytes]:
    if len(pcm) % 4:
        raise ValueError("stereo s16le input must contain complete frames")
    return pcm[0::4] + pcm[1::4], pcm[2::4] + pcm[3::4]


def wav_header() -> bytes:
    buffer = BytesIO()
    with wave.open(buffer, "wb") as output:
        output.setnchannels(1)
        output.setsampwidth(2)
        output.setframerate(16_000)
        output.writeframes(b"")
    return buffer.getvalue()


async def default_connect(url: str, headers: dict[str, str]) -> Any:
    return await websockets.connect(url, additional_headers=headers)


class AzureResultStream:
    def __init__(
        self, conn: Any, channel: str, input_rate: int, clock: Callable[[], float] = time.monotonic
    ):
        self.conn = conn
        self.channel = channel
        self.input_rate = input_rate
        self.clock = clock
        self._closing = False

    async def feed(self, pcm: bytes) -> None:
        await self.conn.send(downsample_mono_s16le(pcm, self.input_rate))

    async def close(self) -> None:
        if self._closing:
            return
        self._closing = True
        await self.conn.close()

    def __aiter__(self) -> "AzureResultStream":
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
            _, _, body = raw.partition("\r\n\r\n")
            message = json.loads(body or raw)
            if message.get("RecognitionStatus") == "Success":
                return map_message(message, channel=self.channel)
            if (
                message.get("RecognitionStatus") in {"Error", "Failure"}
                or message.get("type") == "Error"
            ):
                raise ProviderResetError(str(message))
        raise StopAsyncIteration


class AzureEngine:
    def __init__(
        self,
        config: AzureConfig,
        connect: Connect | None = None,
        clock: Callable[[], float] = time.monotonic,
    ):
        self.config = config
        self._connect = connect or default_connect
        self.clock = clock

    async def start(self, ctx: MeetingContext) -> AzureResultStream:
        connection_id = uuid.uuid4().hex
        conn = await self._connect(
            build_url(self.config, connection_id),
            {"Ocp-Apim-Subscription-Key": self.config.api_key},
        )
        status = conn.response.status_code
        if status in (401, 403):
            raise ProviderAuthError(f"handshake rejected with HTTP {status}")
        if status >= 400:
            raise ProviderResetError(f"handshake failed with HTTP {status}")
        config_frame = {
            "context": {
                "system": {"version": "1.0.0", "name": "SpeechSDK"},
                "os": {"platform": "Python"},
            },
            "recognition": {"outputFormat": "Detailed"},
        }
        await conn.send(
            "Path: speech.config\r\nContent-Type: application/json\r\n\r\n"
            + json.dumps(config_frame)
        )
        await conn.send(wav_header())
        return AzureResultStream(conn, "CALLER", ctx["sample_rate"], clock=self.clock)
