from __future__ import annotations

from collections.abc import Callable, Iterator
from dataclasses import dataclass
from typing import Generic, Self, TypeVar, cast

from .production import PlatformSdkError, SdkErrorCode

_T = TypeVar("_T")


class StreamCursor(str):
    def __new__(cls, value: str) -> Self:
        if not value or len(value) > 512 or "\0" in value:
            raise PlatformSdkError(SdkErrorCode.INVALID_ARGUMENT, "never")
        return cast(Self, str.__new__(cls, value))


@dataclass(frozen=True)
class StreamEvent(Generic[_T]):
    event_id: str
    previous_cursor: StreamCursor
    cursor: StreamCursor
    value: _T


@dataclass(frozen=True)
class StreamPage(Generic[_T]):
    requested_cursor: StreamCursor
    events: tuple[StreamEvent[_T], ...]
    next_cursor: StreamCursor


class ResumableStream(Generic[_T]):
    def __init__(self, cursor: StreamCursor) -> None:
        self._cursor = cursor
        self._seen: set[str] = set()

    @property
    def cursor(self) -> StreamCursor:
        return self._cursor

    def accept(self, page: StreamPage[_T]) -> tuple[StreamEvent[_T], ...]:
        if page.requested_cursor != self._cursor:
            raise PlatformSdkError(SdkErrorCode.DECODE_FAILURE, "never")
        expected = self._cursor
        accepted: list[StreamEvent[_T]] = []
        page_event_ids: set[str] = set()
        for event in page.events:
            if (
                not event.event_id
                or event.previous_cursor != expected
                or event.event_id in self._seen
                or event.event_id in page_event_ids
            ):
                raise PlatformSdkError(SdkErrorCode.DECODE_FAILURE, "never")
            page_event_ids.add(event.event_id)
            accepted.append(event)
            expected = event.cursor
        if page.next_cursor != expected:
            raise PlatformSdkError(SdkErrorCode.DECODE_FAILURE, "never")
        self._seen.update(page_event_ids)
        self._cursor = page.next_cursor
        return tuple(accepted)

    def events(self, source: Callable[[StreamCursor], StreamPage[_T]]) -> Iterator[StreamEvent[_T]]:
        while True:
            yield from self.accept(source(self._cursor))
