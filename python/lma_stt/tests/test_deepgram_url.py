from lma_stt.deepgram import DeepgramConfig, build_url


def test_default_config_produces_documented_query_string():
    url = build_url(DeepgramConfig(api_key="k"))
    assert url == (
        "wss://api.deepgram.com/v1/listen"
        "?encoding=linear16&multichannel=true&channels=2"
        "&sample_rate=48000&model=nova-3&language=multi"
        "&interim_results=true&smart_format=true&diarize=true&endpointing=100"
    )


def test_overrides_reach_the_query_and_unset_endpointing_is_dropped():
    url = build_url(
        DeepgramConfig(
            api_key="k",
            model="nova-2",
            language="en",
            sample_rate=16000,
            endpointing_ms=None,
        )
    )
    assert url == (
        "wss://api.deepgram.com/v1/listen"
        "?encoding=linear16&multichannel=true&channels=2"
        "&sample_rate=16000&model=nova-2&language=en"
        "&interim_results=true&smart_format=true&diarize=true"
    )
