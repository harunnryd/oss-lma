import wave
from pathlib import Path
from typing import Protocol


class RecordingSink(Protocol):
    def feed(self, pcm: bytes) -> None: ...
    def stop(self) -> None: ...


class NullRecordingSink:
    def feed(self, pcm: bytes) -> None:
        return None

    def stop(self) -> None:
        return None


class WavRecordingSink:
    def __init__(self, path: Path) -> None:
        self._file = wave.Wave_write(str(path))
        self._file.setnchannels(2)
        self._file.setsampwidth(2)
        self._file.setframerate(48000)

    def feed(self, pcm: bytes) -> None:
        self._file.writeframes(pcm)

    def stop(self) -> None:
        self._file.close()
