class FakeResponse:
    def __init__(self, status_code: int = 200):
        self.status_code = status_code


class FakeTransport:
    def __init__(
        self,
        messages: list[str] | None = None,
        status_code: int = 200,
        error: Exception | None = None,
    ):
        self.response = FakeResponse(status_code)
        self.messages = list(messages or [])
        self.error = error
        self.sent: list[str | bytes] = []
        self.closed = False

    async def send(self, payload: str | bytes) -> None:
        self.sent.append(payload)

    async def recv(self) -> str:
        if self.messages:
            return self.messages.pop(0)
        if self.error is not None:
            raise self.error
        raise AssertionError("transport exhausted")

    async def close(self) -> None:
        self.closed = True


def fake_connect(conn: FakeTransport, capture: dict | None = None):
    async def connect(url: str, headers: dict[str, str]):
        if capture is not None:
            capture["url"] = url
            capture["headers"] = headers
        return conn

    return connect
