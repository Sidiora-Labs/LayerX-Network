use std::fmt::Write as _;
use x_websearch::canonical::{
    canonical_bytes, canonicalise, collapse_blank_lines, collapse_spaces, content_digest, decode,
    digest_hex, media_type_essence, remove_controls, trim_line_ends, unify_line_breaks,
    CanonicalContent, CanonicalError, ContentKind, CONTENT_DOMAIN,
};

const SEARCH_TEXT: &str =
    "[{\"url\":\"https://paxeer.app/\",\"title\":\"Paxeer\",\"snippet\":\"caf\u{e9}\"}]";
const SEARCH_BYTES: &str = "504158454552585f5745425f434f4e54454e545f5631020000000171000000106170706c69636174696f6e2f6a736f6e00000000000000425b7b2275726c223a2268747470733a2f2f7061786565722e6170702f222c227469746c65223a22506178656572222c22736e6970706574223a22636166c3a9227d5d";
const SEARCH_DIGEST: &str = "e805b79ad7fc1c205e2d72083cfbb086907c7ac684812a2c2b07f807aa3f1d2a";
const EMPTY_KECCAK: &str = "c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

#[test]
fn decoding_follows_the_declared_charset_and_refuses_invalid_input() {
    assert_eq!(decode(b"caf\xc3\xa9", None), Ok("caf\u{e9}".to_owned()));
    assert_eq!(
        decode(b"\xef\xbb\xbfbom", Some("UTF-8")),
        Ok("bom".to_owned())
    );
    assert_eq!(
        decode(b"quoted", Some("\"utf-8\"")),
        Ok("quoted".to_owned())
    );
    assert_eq!(
        decode(b"bad \xff", Some("utf-8")),
        Err(CanonicalError::InvalidEncoding)
    );
    assert_eq!(decode(b"plain", Some("us-ascii")), Ok("plain".to_owned()));
    assert_eq!(
        decode(b"caf\xe9", Some("us-ascii")),
        Err(CanonicalError::InvalidEncoding)
    );
    assert_eq!(
        decode(b"caf\xe9 \x80", Some("iso-8859-1")),
        Ok("caf\u{e9} \u{80}".to_owned())
    );
    assert_eq!(
        decode(b"\x80 \x93q\x94 caf\xe9", Some("windows-1252")),
        Ok("\u{20ac} \u{201c}q\u{201d} caf\u{e9}".to_owned())
    );
    assert_eq!(
        decode(b"\x81", Some("windows-1252")),
        Err(CanonicalError::InvalidEncoding)
    );
    assert_eq!(
        decode(b"text", Some("shift_jis")),
        Err(CanonicalError::UnsupportedCharset)
    );
}

#[test]
fn line_breaks_are_unified() {
    assert_eq!(unify_line_breaks("a\r\nb\rc\nd\r\n\r"), "a\nb\nc\nd\n\n");
}

#[test]
fn control_characters_other_than_lf_and_tab_are_removed() {
    assert_eq!(
        remove_controls("a\u{0}b\u{7}c\td\ne\u{1b}f\u{7f}g\u{85}h\u{9f}i"),
        "abc\td\nefghi"
    );
    assert_eq!(remove_controls("keep\u{a0}\u{200b}"), "keep\u{a0}\u{200b}");
}

#[test]
fn runs_of_spaces_and_tabs_collapse_to_one_space() {
    assert_eq!(collapse_spaces("a  b\t\tc \t d\te"), "a b c d e");
    assert_eq!(collapse_spaces("  lead\n\t\ttab"), " lead\n tab");
    assert_eq!(
        collapse_spaces("nbsp\u{a0}\u{a0}stays"),
        "nbsp\u{a0}\u{a0}stays"
    );
}

#[test]
fn trailing_spaces_are_removed_from_each_line() {
    assert_eq!(trim_line_ends("a  \n b \n\n  \nc "), "a\n b\n\n\nc");
}

#[test]
fn three_or_more_line_breaks_collapse_to_two() {
    assert_eq!(
        collapse_blank_lines("a\nb\n\nc\n\n\nd\n\n\n\n\ne"),
        "a\nb\n\nc\n\nd\n\ne"
    );
}

#[test]
fn leading_and_trailing_whitespace_is_trimmed() {
    assert_eq!(canonicalise("\n\n  text  \n\n"), "text");
    assert_eq!(canonicalise("\t\u{a0}inner  words\u{a0}\n"), "inner words");
}

#[test]
fn the_result_is_in_unicode_nfc() {
    assert_eq!(canonicalise("Cafe\u{301}"), "Caf\u{e9}");
    assert_eq!(canonicalise("\u{212b}ngstr\u{f6}m"), "\u{c5}ngstr\u{f6}m");
    assert_eq!(canonicalise("\u{1e9b}\u{323}"), "\u{1e9b}\u{323}");
}

#[test]
fn canonicalisation_applies_every_step_in_order_and_is_idempotent() {
    let raw = " \r\n Title\t\t here \r\r\r\rbody\u{7} text   \r\n\n\n\nend e\u{301}  \n";
    let expected = "Title here\n\nbody text\n\nend \u{e9}";
    assert_eq!(canonicalise(raw), expected);
    assert_eq!(canonicalise(expected), expected);
}

#[test]
fn media_types_lose_their_parameters_and_case() {
    assert_eq!(
        media_type_essence("Text/HTML; charset=UTF-8"),
        Ok("text/html".to_owned())
    );
    assert_eq!(
        media_type_essence(" application/ld+json "),
        Ok("application/ld+json".to_owned())
    );
    for malformed in [
        "text",
        "/html",
        "text/",
        "text/ht ml",
        "",
        "te\u{e9}xt/html",
    ] {
        assert_eq!(
            media_type_essence(malformed),
            Err(CanonicalError::InvalidMediaType),
            "{malformed}"
        );
    }
}

#[test]
fn canonical_bytes_follow_the_layout_and_match_the_committed_vector() -> Result<(), CanonicalError>
{
    assert_eq!(ContentKind::Fetch.byte(), 1);
    assert_eq!(ContentKind::Search.byte(), 2);
    assert_eq!(ContentKind::from_byte(2), Some(ContentKind::Search));
    assert_eq!(ContentKind::from_byte(3), None);

    let bytes = canonical_bytes(
        ContentKind::Search,
        b"q",
        "Application/JSON; charset=utf-8",
        SEARCH_TEXT,
    )?;
    assert_eq!(hex(&bytes), SEARCH_BYTES);
    assert_eq!(&bytes[..CONTENT_DOMAIN.len()], b"PAXEERX_WEB_CONTENT_V1");
    assert_eq!(digest_hex(&content_digest(&bytes)), SEARCH_DIGEST);
    assert_eq!(digest_hex(&content_digest(b"")), EMPTY_KECCAK);

    let mut expected = b"PAXEERX_WEB_CONTENT_V1".to_vec();
    expected.push(1);
    expected.extend_from_slice(&[0, 0, 0, 3]);
    expected.extend_from_slice(b"url");
    expected.extend_from_slice(&[0, 0, 0, 10]);
    expected.extend_from_slice(b"text/plain");
    expected.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 2]);
    expected.extend_from_slice(b"hi");
    assert_eq!(
        canonical_bytes(ContentKind::Fetch, b"url", "text/plain", "hi")?,
        expected
    );
    assert_eq!(
        canonical_bytes(ContentKind::Fetch, b"url", "plain", "hi"),
        Err(CanonicalError::InvalidMediaType)
    );
    Ok(())
}

#[test]
fn canonical_bytes_parse_back_and_nothing_else_does() -> Result<(), CanonicalError> {
    let bytes = canonical_bytes(ContentKind::Search, b"q", "application/json", SEARCH_TEXT)?;
    let parsed = CanonicalContent::parse(&bytes)?;
    assert_eq!(parsed.kind, ContentKind::Search);
    assert_eq!(parsed.payload, b"q");
    assert_eq!(parsed.media_type, "application/json");
    assert_eq!(parsed.text, SEARCH_TEXT);

    let mut trailing = bytes.clone();
    trailing.push(0);
    let mut short = bytes.clone();
    short.pop();
    let mut wrong_kind = bytes.clone();
    wrong_kind[CONTENT_DOMAIN.len()] = 3;
    let mut wrong_domain = bytes.clone();
    wrong_domain[0] = b'Q';
    let upper = canonical_bytes(ContentKind::Fetch, b"", "text/plain", "x")?
        .into_iter()
        .map(|byte| if byte == b'p' { b'P' } else { byte })
        .collect::<Vec<_>>();
    for malformed in [trailing, short, wrong_kind, wrong_domain, upper, Vec::new()] {
        assert_eq!(
            CanonicalContent::parse(&malformed),
            Err(CanonicalError::Malformed)
        );
    }
    Ok(())
}
