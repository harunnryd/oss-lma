import asyncio
import signal

from lma_stt.fake import FakeEngine

from sidecar.server import BindFailed, run_server


def default_engine_factory(ctx):
    return FakeEngine(script=[])


async def main() -> int:
    stop = asyncio.Event()
    loop = asyncio.get_running_loop()
    for sig in (signal.SIGINT, signal.SIGTERM):
        loop.add_signal_handler(sig, stop.set)
    try:
        await run_server(default_engine_factory, stop=stop)
    except BindFailed:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(asyncio.run(main()))
