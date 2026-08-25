import asyncio
from dataclasses import replace

from lma_stt.types import (
    MeetingContext,
    ProviderAuthError,
    ProviderResetError,
    Result,
    WordItem,
)

CHUNK_BYTES = 19_200


def two_speaker_script() -> list[dict]:
    return [
        {
            "result_id": "fake-r0",
            "at_chunk": 20,
            "channel": "CALLER",
            "words": ["can", "everyone", "see", "the", "updated", "forecast"],
            "speaker": None,
            "final": False,
        },
        {
            "result_id": "fake-r0",
            "at_chunk": 25,
            "channel": "CALLER",
            "words": [],
            "speaker": "spk_0",
            "final": True,
        },
        {
            "result_id": "fake-r1",
            "at_chunk": 40,
            "channel": "AGENT",
            "words": ["yes", "and", "the", "variance", "explains", "the", "hiring", "delay"],
            "speaker": None,
            "final": False,
        },
        {
            "result_id": "fake-r1",
            "at_chunk": 45,
            "channel": "AGENT",
            "words": [],
            "speaker": "spk_1",
            "final": True,
        },
    ]


class FakeEngine:
    def __init__(
        self,
        script: list[dict],
        auth_failure: bool = False,
        reset_after_chunks: int | None = None,
    ):
        self.script = script
        self.auth_failure = auth_failure
        self.reset_after_chunks = reset_after_chunks
        self.started_ctx: MeetingContext | None = None
        self.closed = False

    async def start(self, ctx: MeetingContext) -> "FakeResultStream":
        if self.auth_failure:
            raise ProviderAuthError("scripted auth failure")
        self.started_ctx = ctx
        return FakeResultStream(self, ctx["sample_rate"] * 4 // 10)


class FakeResultStream:
    def __init__(self, engine: FakeEngine, chunk_bytes: int = CHUNK_BYTES):
        self.engine = engine
        self.chunk_bytes = chunk_bytes
        self.chunk_count = 0
        self._pending: list[Result] = []
        self._buffered: dict[str, list[WordItem]] = {}
        self._fired = 0
        self._closed = False
        self._new_item = asyncio.Event()

    async def feed(self, pcm: bytes) -> None:
        if len(pcm) != self.chunk_bytes:
            raise ValueError(f"expected {self.chunk_bytes}-byte stereo s16le chunk, got {len(pcm)}")
        self.chunk_count += 1
        if (
            self.engine.reset_after_chunks is not None
            and self.chunk_count >= self.engine.reset_after_chunks
        ):
            raise ProviderResetError(f"scripted reset after chunk {self.chunk_count}")
        while (
            self._fired < len(self.engine.script)
            and self.engine.script[self._fired]["at_chunk"] <= self.chunk_count
        ):
            self._emit(self.engine.script[self._fired])
            self._fired += 1

    def _emit(self, stage: dict) -> None:
        result_id = stage["result_id"]
        base = (stage["at_chunk"] - len(stage["words"])) * 0.1
        for offset, word in enumerate(stage["words"]):
            item = WordItem(
                content=word,
                type="pronunciation",
                start_time=round(base + offset * 0.1, 6),
                end_time=round(base + (offset + 1) * 0.1, 6),
                speaker=None,
                channel=stage["channel"],
                result_id=result_id,
            )
            self._buffered.setdefault(result_id, []).append(item)
        if stage["final"]:
            items = self._buffered.pop(result_id)
            if stage.get("speaker"):
                items = [replace(item, speaker=stage["speaker"]) for item in items]
            self._pending.append({"result_id": result_id, "is_partial": False, "items": items})
        else:
            self._pending.append(
                {"result_id": result_id, "is_partial": True, "items": list(self._buffered[result_id])}
            )
        self._new_item.set()

    async def close(self) -> None:
        self.engine.closed = True
        self._closed = True
        self._new_item.set()

    def __aiter__(self) -> "FakeResultStream":
        return self

    async def __anext__(self) -> Result:
        while not self._pending:
            if self._closed:
                raise StopAsyncIteration
            self._new_item.clear()
            await self._new_item.wait()
        return self._pending.pop(0)
