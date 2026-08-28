import asyncio
import json
import time
import uuid
import wave
from collections.abc import Awaitable, Callable
from dataclasses import dataclass
from io import BytesIO
from typing import Any, Literal

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


def map_message(message: dict[str, Any], *, channel: Literal["CALLER", "AGENT"]) -> Result:
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
            channel=channel,
            result_id=result_id,
        )
        for word, content in zip(words, rendered, strict=True)
    ]
    return {"result_id": result_id, "is_partial": False, "items": items}


def map_messages(
    messages: list[dict[str, Any]], *, channel: Literal["CALLER", "AGENT"] = "CALLER"
) -> list[Result]:
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


def wav_header(data_size: int) -> bytes:
    buffer = BytesIO()
    with wave.open(buffer, "wb") as output:
        output.setnchannels(1)
        output.setsampwidth(2)
        output.setframerate(16_000)
        output.writeframes(b"\0" * data_size)
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
        self.request_id = uuid.uuid4().hex
        self.first_audio = True

    async def feed(self, pcm: bytes) -> None:
        audio = downsample_mono_s16le(pcm, self.input_rate)
        header = (
            f"Path: audio\r\nX-RequestId: {self.request_id}\r\nContent-Type: audio/x-wav\r\n\r\n"
        ).encode()
        if self.first_audio:
            await self.conn.send(header + wav_header(0))
            self.first_audio = False
        await self.conn.send(header + audio)

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
            headers, _, body = raw.partition("\r\n\r\n")
            message = json.loads(body or raw)
            if "Path: speech.hypothesis" in headers:
                offset = int(message.get("Offset", 0))
                duration = int(message.get("Duration", 0))
                result_id = str(message.get("Id", f"{offset}-{duration}"))
                text = str(message.get("Text", ""))
                return {
                    "result_id": result_id,
                    "is_partial": True,
                    "items": [
                        WordItem(
                            text,
                            "pronunciation",
                            offset / _TICKS_PER_SECOND,
                            (offset + duration) / _TICKS_PER_SECOND,
                            None,
                            self.channel,
                            result_id,
                        )
                    ]
                    if text
                    else [],
                }
            if message.get("RecognitionStatus") == "Success":
                return map_message(message, channel=self.channel)
            if (
                message.get("RecognitionStatus") in {"Error", "Failure"}
                or message.get("type") == "Error"
            ):
                raise ProviderResetError(str(message))
        raise StopAsyncIteration


class _AzureChannelResultStream:
    def __init__(self, streams: list[AzureResultStream]):
        self.streams = streams
        self._pending: dict[asyncio.Task[Result], AzureResultStream] = {}
        self._ready: list[Result] = []

    async def feed(self, pcm: bytes) -> None:
        for stream, channel_pcm in zip(self.streams, deinterleave_s16le(pcm), strict=True):
            await stream.feed(channel_pcm)

    async def close(self) -> None:
        await asyncio.gather(*(stream.close() for stream in self.streams))

    def __aiter__(self):
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

    async def start(self, ctx: MeetingContext) -> _AzureChannelResultStream:
        config_frame = {
            "context": {
                "system": {"version": "1.0.0", "name": "SpeechSDK"},
                "os": {"platform": "Python"},
            },
            "recognition": {"outputFormat": "Detailed"},
        }
        streams = []
        try:
            for channel in ("CALLER", "AGENT"):
                connection_id = uuid.uuid4().hex
                try:
                    conn = await self._connect(
                        build_url(self.config, connection_id),
                        {"Ocp-Apim-Subscription-Key": self.config.api_key},
                    )
                except Exception as exc:
                    raise _connection_error(exc) from exc
                stream = AzureResultStream(conn, channel, ctx["sample_rate"], clock=self.clock)
                streams.append(stream)
                status = conn.response.status_code
                if status in (401, 403):
                    raise ProviderAuthError(f"handshake rejected with HTTP {status}")
                if status >= 400:
                    raise ProviderResetError(f"handshake failed with HTTP {status}")
                await conn.send(
                    "Path: speech.config\r\nContent-Type: application/json\r\n\r\n"
                    + json.dumps(config_frame)
                )
        except Exception:
            await asyncio.gather(*(stream.close() for stream in streams))
            raise
        return _AzureChannelResultStream(streams)
