import asyncio
import random
import secrets
import sys
from collections.abc import Callable
from http import HTTPStatus
from typing import TextIO
from urllib.parse import parse_qs, urlsplit

from websockets.asyncio.server import ServerConnection, serve
from websockets.http11 import Request, Response

from sidecar.session import Session
from sidecar.storage.persistence import PersistenceWriter

MAX_BIND_ATTEMPTS = 10
WS_PATH = "/ws"


class BindFailed(Exception):
    pass


def authorize(path: str, token: str) -> bool:
    split = urlsplit(path)
    if split.path != WS_PATH:
        return False
    supplied = parse_qs(split.query).get("token", [None])[0]
    return secrets.compare_digest(supplied or "", token)


def _gate(token: str) -> Callable[[ServerConnection, Request], Response | None]:
    def process_request(connection: ServerConnection, request: Request) -> Response | None:
        split = urlsplit(request.path)
        if split.path != WS_PATH:
            return connection.respond(HTTPStatus.NOT_FOUND, HTTPStatus.NOT_FOUND.phrase)
        supplied = parse_qs(split.query).get("token", [None])[0]
        if not secrets.compare_digest(supplied or "", token):
            return connection.respond(HTTPStatus.UNAUTHORIZED, HTTPStatus.UNAUTHORIZED.phrase)
        return None

    return process_request


def _handler(
    engine_factory: Callable,
    sessions: set,
    db_writer: PersistenceWriter | None,
) -> Callable[[ServerConnection], object]:
    async def handle(connection: ServerConnection) -> None:
        session = Session(connection, engine_factory, db=db_writer)
        sessions.add(session)
        try:
            await session.run()
        finally:
            sessions.discard(session)

    return handle


async def run_server(
    engine_factory: Callable,
    stop: asyncio.Event | None = None,
    ready_sink: TextIO | None = None,
    *,
    db_writer: PersistenceWriter | None = None,
    record_meeting: bool = False,
) -> tuple[int, str]:
    sink = ready_sink if ready_sink is not None else sys.stdout
    token = secrets.token_hex(16)
    sessions: set = set()
    server = None
    port = 0
    for attempt in range(MAX_BIND_ATTEMPTS):
        try:
            server = await serve(
                _handler(engine_factory, sessions, db_writer),
                host="127.0.0.1",
                port=port,
                process_request=_gate(token),
            )
            break
        except OSError:
            if attempt == MAX_BIND_ATTEMPTS - 1:
                raise BindFailed() from None
            port = random.randint(49152, 65535)
    bound_port = server.sockets[0].getsockname()[1]
    sink.write(f"SIDECAR_READY port={bound_port} token={token}\n")
    sink.flush()
    try:
        if stop is None:
            await asyncio.Event().wait()
        else:
            await stop.wait()
    finally:
        for session in list(sessions):
            await session.shutdown()
        server.close(code=1000)
        await server.wait_closed()
    return bound_port, token
