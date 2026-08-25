from dataclasses import dataclass


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
