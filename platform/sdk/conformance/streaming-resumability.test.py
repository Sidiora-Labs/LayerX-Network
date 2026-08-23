import pytest
from layerx_sdk import (
    ResumableStream,
    StreamPage,
    StreamCursor,
    StreamEvent,
    PlatformSdkError,
    SdkErrorCode,
)


class TestStreamCursorHygiene:
    def test_constructs_valid_cursors(self):
        cursor = StreamCursor("cursor-123")
        assert cursor is not None

    def test_refuses_empty_cursors(self):
        with pytest.raises(PlatformSdkError) as exc_info:
            StreamCursor("")
        assert exc_info.value.code == SdkErrorCode.INVALID_ARGUMENT

    def test_refuses_overlong_cursors(self):
        overlong = "a" * 513
        with pytest.raises(PlatformSdkError):
            StreamCursor(overlong)

    def test_refuses_nul_containing_cursors(self):
        with pytest.raises(PlatformSdkError):
            StreamCursor("has\0null")


class TestResumableStreamNoGapNoDuplicate:
    def test_accepts_empty_page(self):
        initial_cursor = StreamCursor("c0")
        stream = ResumableStream(initial_cursor)
        page = StreamPage(
            requested_cursor=initial_cursor,
            events=(),
            next_cursor=initial_cursor,
        )
        accepted = stream.accept(page)
        assert len(accepted) == 0
        assert stream.cursor == initial_cursor

    def test_accepts_single_event(self):
        initial_cursor = StreamCursor("c0")
        stream = ResumableStream(initial_cursor)
        next_cursor = StreamCursor("c1")
        page = StreamPage(
            requested_cursor=initial_cursor,
            events=(
                StreamEvent(
                    event_id="e1",
                    previous_cursor=initial_cursor,
                    cursor=next_cursor,
                    value="first",
                ),
            ),
            next_cursor=next_cursor,
        )
        accepted = stream.accept(page)
        assert len(accepted) == 1
        assert accepted[0].value == "first"
        assert stream.cursor == next_cursor

    def test_accepts_multiple_events_forming_chain(self):
        initial_cursor = StreamCursor("c0")
        stream = ResumableStream(initial_cursor)
        c1 = StreamCursor("c1")
        c2 = StreamCursor("c2")
        c3 = StreamCursor("c3")
        page = StreamPage(
            requested_cursor=initial_cursor,
            events=(
                StreamEvent(
                    event_id="e1",
                    previous_cursor=initial_cursor,
                    cursor=c1,
                    value="first",
                ),
                StreamEvent(
                    event_id="e2",
                    previous_cursor=c1,
                    cursor=c2,
                    value="second",
                ),
                StreamEvent(
                    event_id="e3",
                    previous_cursor=c2,
                    cursor=c3,
                    value="third",
                ),
            ),
            next_cursor=c3,
        )
        accepted = stream.accept(page)
        assert len(accepted) == 3
        assert stream.cursor == c3

    def test_refuses_page_with_wrong_requested_cursor(self):
        initial_cursor = StreamCursor("c0")
        stream = ResumableStream(initial_cursor)
        wrong_cursor = StreamCursor("c-wrong")
        page = StreamPage(
            requested_cursor=wrong_cursor,
            events=(),
            next_cursor=wrong_cursor,
        )
        with pytest.raises(PlatformSdkError) as exc_info:
            stream.accept(page)
        assert exc_info.value.code == SdkErrorCode.DECODE_FAILURE

    def test_refuses_gap_in_cursor_chain(self):
        initial_cursor = StreamCursor("c0")
        stream = ResumableStream(initial_cursor)
        c1 = StreamCursor("c1")
        c2 = StreamCursor("c2")
        page = StreamPage(
            requested_cursor=initial_cursor,
            events=(
                StreamEvent(
                    event_id="e1",
                    previous_cursor=c1,
                    cursor=c2,
                    value="skipped",
                ),
            ),
            next_cursor=c2,
        )
        with pytest.raises(PlatformSdkError):
            stream.accept(page)

    def test_refuses_duplicate_event_ids_across_pages(self):
        initial_cursor = StreamCursor("c0")
        stream = ResumableStream(initial_cursor)
        c1 = StreamCursor("c1")
        c2 = StreamCursor("c2")
        first_page = StreamPage(
            requested_cursor=initial_cursor,
            events=(
                StreamEvent(
                    event_id="e1",
                    previous_cursor=initial_cursor,
                    cursor=c1,
                    value="first",
                ),
            ),
            next_cursor=c1,
        )
        stream.accept(first_page)
        duplicate_page = StreamPage(
            requested_cursor=c1,
            events=(
                StreamEvent(
                    event_id="e1",
                    previous_cursor=c1,
                    cursor=c2,
                    value="duplicate",
                ),
            ),
            next_cursor=c2,
        )
        with pytest.raises(PlatformSdkError):
            stream.accept(duplicate_page)

    def test_refuses_duplicate_event_ids_within_page(self):
        initial_cursor = StreamCursor("c0")
        stream = ResumableStream(initial_cursor)
        c1 = StreamCursor("c1")
        c2 = StreamCursor("c2")
        page = StreamPage(
            requested_cursor=initial_cursor,
            events=(
                StreamEvent(
                    event_id="e1",
                    previous_cursor=initial_cursor,
                    cursor=c1,
                    value="first",
                ),
                StreamEvent(
                    event_id="e1",
                    previous_cursor=c1,
                    cursor=c2,
                    value="duplicate",
                ),
            ),
            next_cursor=c2,
        )
        with pytest.raises(PlatformSdkError):
            stream.accept(page)

    def test_refuses_empty_event_id(self):
        initial_cursor = StreamCursor("c0")
        stream = ResumableStream(initial_cursor)
        c1 = StreamCursor("c1")
        page = StreamPage(
            requested_cursor=initial_cursor,
            events=(
                StreamEvent(
                    event_id="",
                    previous_cursor=initial_cursor,
                    cursor=c1,
                    value="no-id",
                ),
            ),
            next_cursor=c1,
        )
        with pytest.raises(PlatformSdkError):
            stream.accept(page)

    def test_refuses_mismatched_next_cursor(self):
        initial_cursor = StreamCursor("c0")
        stream = ResumableStream(initial_cursor)
        c1 = StreamCursor("c1")
        c2 = StreamCursor("c2")
        page = StreamPage(
            requested_cursor=initial_cursor,
            events=(
                StreamEvent(
                    event_id="e1",
                    previous_cursor=initial_cursor,
                    cursor=c1,
                    value="first",
                ),
            ),
            next_cursor=c2,
        )
        with pytest.raises(PlatformSdkError):
            stream.accept(page)

    def test_supports_resumption_after_disconnection(self):
        initial_cursor = StreamCursor("c0")
        stream = ResumableStream(initial_cursor)
        c1 = StreamCursor("c1")
        c2 = StreamCursor("c2")
        first_page = StreamPage(
            requested_cursor=initial_cursor,
            events=(
                StreamEvent(
                    event_id="e1",
                    previous_cursor=initial_cursor,
                    cursor=c1,
                    value="before-disconnect",
                ),
            ),
            next_cursor=c1,
        )
        stream.accept(first_page)
        resumed_page = StreamPage(
            requested_cursor=c1,
            events=(
                StreamEvent(
                    event_id="e2",
                    previous_cursor=c1,
                    cursor=c2,
                    value="after-reconnect",
                ),
            ),
            next_cursor=c2,
        )
        accepted = stream.accept(resumed_page)
        assert len(accepted) == 1
        assert accepted[0].value == "after-reconnect"
        assert stream.cursor == c2
