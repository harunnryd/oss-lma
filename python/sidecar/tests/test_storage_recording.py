import struct
import wave

from sidecar.storage.recording import NullRecordingSink, WavRecordingSink


def test_wav_sink_creates_valid_wav_header(tmp_path):
    sink = WavRecordingSink(tmp_path / "out.wav")
    sink.feed(b"\x00" * 19200)
    sink.stop()
    with wave.open(str(tmp_path / "out.wav"), "rb") as reader:
        assert reader.getnchannels() == 2
        assert reader.getsampwidth() == 2
        assert reader.getframerate() == 48000


def test_wav_sink_writes_pcm_payload(tmp_path):
    sink = WavRecordingSink(tmp_path / "out.wav")
    pcm = struct.pack("<hh", 100, -100) * 4800
    sink.feed(pcm)
    sink.stop()
    with wave.open(str(tmp_path / "out.wav"), "rb") as reader:
        frames = reader.readframes(9600)
        assert frames[:2] == b"\x64\x00"
        assert frames[2:4] == b"\x9c\xff"


def test_wav_sink_appends_across_feeds(tmp_path):
    sink = WavRecordingSink(tmp_path / "out.wav")
    sink.feed(b"\x00" * 4800)
    sink.feed(b"\x00" * 4800)
    sink.stop()
    with wave.open(str(tmp_path / "out.wav"), "rb") as reader:
        assert reader.getnframes() == 2400


def test_null_sink_is_no_op(tmp_path):
    sink = NullRecordingSink()
    sink.feed(b"\x00" * 19200)
    sink.stop()
    assert not (tmp_path / "out.wav").exists()


def test_wav_sink_rejects_non_48khz_assumption_via_documentation(tmp_path):
    sink = WavRecordingSink(tmp_path / "out.wav")
    sink.feed(b"\x00" * 19200)
    sink.stop()
    with wave.open(str(tmp_path / "out.wav"), "rb") as reader:
        assert reader.getframerate() == 48000