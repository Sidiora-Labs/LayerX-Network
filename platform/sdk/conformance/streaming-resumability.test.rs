#[cfg(test)]
mod streaming_resumability_tests {
    use layerx_sdk::production::{
        ProductionError, ResumableStream, SdkErrorCode, StreamCursor, StreamEvent, StreamPage,
    };

    #[test]
    fn stream_cursor_constructs_valid_cursors() {
        let cursor = StreamCursor::new("cursor-123");
        assert!(cursor.is_ok());
    }

    #[test]
    fn stream_cursor_refuses_empty_cursors() {
        let cursor = StreamCursor::new("");
        assert!(cursor.is_err());
        assert_eq!(cursor.unwrap_err().code, SdkErrorCode::InvalidArgument);
    }

    #[test]
    fn stream_cursor_refuses_overlong_cursors() {
        let overlong = "a".repeat(513);
        let cursor = StreamCursor::new(overlong);
        assert!(cursor.is_err());
    }

    #[test]
    fn stream_cursor_refuses_nul_containing_cursors() {
        let cursor = StreamCursor::new("has\0null");
        assert!(cursor.is_err());
    }

    #[test]
    fn resumable_stream_accepts_empty_page() {
        let initial_cursor = StreamCursor::new("c0").unwrap();
        let mut stream = ResumableStream::new(initial_cursor.clone());
        let page = StreamPage {
            requested_cursor: initial_cursor.clone(),
            events: vec![],
            next_cursor: initial_cursor.clone(),
        };
        let accepted = stream.accept(page).unwrap();
        assert_eq!(accepted.len(), 0);
        assert_eq!(stream.cursor().as_str(), "c0");
    }

    #[test]
    fn resumable_stream_accepts_single_event() {
        let initial_cursor = StreamCursor::new("c0").unwrap();
        let mut stream = ResumableStream::new(initial_cursor.clone());
        let next_cursor = StreamCursor::new("c1").unwrap();
        let page = StreamPage {
            requested_cursor: initial_cursor.clone(),
            events: vec![StreamEvent {
                event_id: "e1".to_string(),
                previous_cursor: initial_cursor.clone(),
                cursor: next_cursor.clone(),
                value: "first",
            }],
            next_cursor: next_cursor.clone(),
        };
        let accepted = stream.accept(page).unwrap();
        assert_eq!(accepted.len(), 1);
        assert_eq!(accepted[0].value, "first");
        assert_eq!(stream.cursor().as_str(), "c1");
    }

    #[test]
    fn resumable_stream_accepts_multiple_events_forming_chain() {
        let initial_cursor = StreamCursor::new("c0").unwrap();
        let mut stream = ResumableStream::new(initial_cursor.clone());
        let c1 = StreamCursor::new("c1").unwrap();
        let c2 = StreamCursor::new("c2").unwrap();
        let c3 = StreamCursor::new("c3").unwrap();
        let page = StreamPage {
            requested_cursor: initial_cursor.clone(),
            events: vec![
                StreamEvent {
                    event_id: "e1".to_string(),
                    previous_cursor: initial_cursor.clone(),
                    cursor: c1.clone(),
                    value: "first",
                },
                StreamEvent {
                    event_id: "e2".to_string(),
                    previous_cursor: c1.clone(),
                    cursor: c2.clone(),
                    value: "second",
                },
                StreamEvent {
                    event_id: "e3".to_string(),
                    previous_cursor: c2.clone(),
                    cursor: c3.clone(),
                    value: "third",
                },
            ],
            next_cursor: c3.clone(),
        };
        let accepted = stream.accept(page).unwrap();
        assert_eq!(accepted.len(), 3);
        assert_eq!(stream.cursor().as_str(), "c3");
    }

    #[test]
    fn resumable_stream_refuses_page_with_wrong_requested_cursor() {
        let initial_cursor = StreamCursor::new("c0").unwrap();
        let mut stream = ResumableStream::new(initial_cursor.clone());
        let wrong_cursor = StreamCursor::new("c-wrong").unwrap();
        let page = StreamPage {
            requested_cursor: wrong_cursor.clone(),
            events: vec![],
            next_cursor: wrong_cursor.clone(),
        };
        let result = stream.accept(page);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, SdkErrorCode::DecodeFailure);
    }

    #[test]
    fn resumable_stream_refuses_gap_in_cursor_chain() {
        let initial_cursor = StreamCursor::new("c0").unwrap();
        let mut stream = ResumableStream::new(initial_cursor.clone());
        let c1 = StreamCursor::new("c1").unwrap();
        let c2 = StreamCursor::new("c2").unwrap();
        let page = StreamPage {
            requested_cursor: initial_cursor.clone(),
            events: vec![StreamEvent {
                event_id: "e1".to_string(),
                previous_cursor: c1.clone(),
                cursor: c2.clone(),
                value: "skipped",
            }],
            next_cursor: c2.clone(),
        };
        let result = stream.accept(page);
        assert!(result.is_err());
    }

    #[test]
    fn resumable_stream_refuses_duplicate_event_ids_across_pages() {
        let initial_cursor = StreamCursor::new("c0").unwrap();
        let mut stream = ResumableStream::new(initial_cursor.clone());
        let c1 = StreamCursor::new("c1").unwrap();
        let c2 = StreamCursor::new("c2").unwrap();
        let first_page = StreamPage {
            requested_cursor: initial_cursor.clone(),
            events: vec![StreamEvent {
                event_id: "e1".to_string(),
                previous_cursor: initial_cursor.clone(),
                cursor: c1.clone(),
                value: "first",
            }],
            next_cursor: c1.clone(),
        };
        stream.accept(first_page).unwrap();
        let duplicate_page = StreamPage {
            requested_cursor: c1.clone(),
            events: vec![StreamEvent {
                event_id: "e1".to_string(),
                previous_cursor: c1.clone(),
                cursor: c2.clone(),
                value: "duplicate",
            }],
            next_cursor: c2.clone(),
        };
        let result = stream.accept(duplicate_page);
        assert!(result.is_err());
    }

    #[test]
    fn resumable_stream_refuses_duplicate_event_ids_within_page() {
        let initial_cursor = StreamCursor::new("c0").unwrap();
        let mut stream = ResumableStream::new(initial_cursor.clone());
        let c1 = StreamCursor::new("c1").unwrap();
        let c2 = StreamCursor::new("c2").unwrap();
        let page = StreamPage {
            requested_cursor: initial_cursor.clone(),
            events: vec![
                StreamEvent {
                    event_id: "e1".to_string(),
                    previous_cursor: initial_cursor.clone(),
                    cursor: c1.clone(),
                    value: "first",
                },
                StreamEvent {
                    event_id: "e1".to_string(),
                    previous_cursor: c1.clone(),
                    cursor: c2.clone(),
                    value: "duplicate",
                },
            ],
            next_cursor: c2.clone(),
        };
        let result = stream.accept(page);
        assert!(result.is_err());
    }

    #[test]
    fn resumable_stream_refuses_empty_event_id() {
        let initial_cursor = StreamCursor::new("c0").unwrap();
        let mut stream = ResumableStream::new(initial_cursor.clone());
        let c1 = StreamCursor::new("c1").unwrap();
        let page = StreamPage {
            requested_cursor: initial_cursor.clone(),
            events: vec![StreamEvent {
                event_id: String::new(),
                previous_cursor: initial_cursor.clone(),
                cursor: c1.clone(),
                value: "no-id",
            }],
            next_cursor: c1.clone(),
        };
        let result = stream.accept(page);
        assert!(result.is_err());
    }

    #[test]
    fn resumable_stream_refuses_mismatched_next_cursor() {
        let initial_cursor = StreamCursor::new("c0").unwrap();
        let mut stream = ResumableStream::new(initial_cursor.clone());
        let c1 = StreamCursor::new("c1").unwrap();
        let c2 = StreamCursor::new("c2").unwrap();
        let page = StreamPage {
            requested_cursor: initial_cursor.clone(),
            events: vec![StreamEvent {
                event_id: "e1".to_string(),
                previous_cursor: initial_cursor.clone(),
                cursor: c1.clone(),
                value: "first",
            }],
            next_cursor: c2.clone(),
        };
        let result = stream.accept(page);
        assert!(result.is_err());
    }

    #[test]
    fn resumable_stream_supports_resumption_after_disconnection() {
        let initial_cursor = StreamCursor::new("c0").unwrap();
        let mut stream = ResumableStream::new(initial_cursor.clone());
        let c1 = StreamCursor::new("c1").unwrap();
        let c2 = StreamCursor::new("c2").unwrap();
        let first_page = StreamPage {
            requested_cursor: initial_cursor.clone(),
            events: vec![StreamEvent {
                event_id: "e1".to_string(),
                previous_cursor: initial_cursor.clone(),
                cursor: c1.clone(),
                value: "before-disconnect",
            }],
            next_cursor: c1.clone(),
        };
        stream.accept(first_page).unwrap();
        let resumed_page = StreamPage {
            requested_cursor: c1.clone(),
            events: vec![StreamEvent {
                event_id: "e2".to_string(),
                previous_cursor: c1.clone(),
                cursor: c2.clone(),
                value: "after-reconnect",
            }],
            next_cursor: c2.clone(),
        };
        let accepted = stream.accept(resumed_page).unwrap();
        assert_eq!(accepted.len(), 1);
        assert_eq!(accepted[0].value, "after-reconnect");
        assert_eq!(stream.cursor().as_str(), "c2");
    }
}
