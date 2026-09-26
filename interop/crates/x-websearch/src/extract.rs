use crate::canonical::{self, CanonicalError};

/// The media types a fetch accepts.
pub const ACCEPTED_MEDIA_TYPES: [&str; 3] = ["text/html", "text/plain", "application/json"];

const DROPPED: [&str; 4] = ["script", "style", "noscript", "template"];
const RAW_TEXT: [&str; 2] = ["script", "style"];

const BLOCKS: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "body",
    "br",
    "caption",
    "dd",
    "details",
    "dialog",
    "dir",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hgroup",
    "hr",
    "html",
    "legend",
    "li",
    "main",
    "menu",
    "nav",
    "ol",
    "optgroup",
    "option",
    "p",
    "pre",
    "section",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "ul",
];

const ENTITIES: &[(&str, char)] = &[
    ("AElig", '\u{c6}'),
    ("Aacute", '\u{c1}'),
    ("Agrave", '\u{c0}'),
    ("Aring", '\u{c5}'),
    ("Atilde", '\u{c3}'),
    ("Auml", '\u{c4}'),
    ("Ccedil", '\u{c7}'),
    ("Eacute", '\u{c9}'),
    ("Egrave", '\u{c8}'),
    ("Iacute", '\u{cd}'),
    ("Ntilde", '\u{d1}'),
    ("Oacute", '\u{d3}'),
    ("Ouml", '\u{d6}'),
    ("Oslash", '\u{d8}'),
    ("Uacute", '\u{da}'),
    ("Uuml", '\u{dc}'),
    ("aacute", '\u{e1}'),
    ("acirc", '\u{e2}'),
    ("acute", '\u{b4}'),
    ("aelig", '\u{e6}'),
    ("agrave", '\u{e0}'),
    ("alpha", '\u{3b1}'),
    ("amp", '&'),
    ("apos", '\''),
    ("aring", '\u{e5}'),
    ("atilde", '\u{e3}'),
    ("auml", '\u{e4}'),
    ("bdquo", '\u{201e}'),
    ("beta", '\u{3b2}'),
    ("brvbar", '\u{a6}'),
    ("bull", '\u{2022}'),
    ("ccedil", '\u{e7}'),
    ("cedil", '\u{b8}'),
    ("cent", '\u{a2}'),
    ("copy", '\u{a9}'),
    ("curren", '\u{a4}'),
    ("dagger", '\u{2020}'),
    ("darr", '\u{2193}'),
    ("deg", '\u{b0}'),
    ("delta", '\u{3b4}'),
    ("divide", '\u{f7}'),
    ("eacute", '\u{e9}'),
    ("ecirc", '\u{ea}'),
    ("egrave", '\u{e8}'),
    ("ensp", '\u{2002}'),
    ("emsp", '\u{2003}'),
    ("euml", '\u{eb}'),
    ("euro", '\u{20ac}'),
    ("frac12", '\u{bd}'),
    ("frac14", '\u{bc}'),
    ("frac34", '\u{be}'),
    ("gamma", '\u{3b3}'),
    ("ge", '\u{2265}'),
    ("gt", '>'),
    ("harr", '\u{2194}'),
    ("hellip", '\u{2026}'),
    ("iacute", '\u{ed}'),
    ("icirc", '\u{ee}'),
    ("iexcl", '\u{a1}'),
    ("igrave", '\u{ec}'),
    ("infin", '\u{221e}'),
    ("iquest", '\u{bf}'),
    ("iuml", '\u{ef}'),
    ("laquo", '\u{ab}'),
    ("larr", '\u{2190}'),
    ("ldquo", '\u{201c}'),
    ("le", '\u{2264}'),
    ("lambda", '\u{3bb}'),
    ("lsaquo", '\u{2039}'),
    ("lsquo", '\u{2018}'),
    ("lt", '<'),
    ("macr", '\u{af}'),
    ("mdash", '\u{2014}'),
    ("micro", '\u{b5}'),
    ("middot", '\u{b7}'),
    ("mu", '\u{3bc}'),
    ("nbsp", '\u{a0}'),
    ("ndash", '\u{2013}'),
    ("ne", '\u{2260}'),
    ("not", '\u{ac}'),
    ("ntilde", '\u{f1}'),
    ("oacute", '\u{f3}'),
    ("ocirc", '\u{f4}'),
    ("ograve", '\u{f2}'),
    ("ordf", '\u{aa}'),
    ("ordm", '\u{ba}'),
    ("oslash", '\u{f8}'),
    ("ouml", '\u{f6}'),
    ("para", '\u{b6}'),
    ("pi", '\u{3c0}'),
    ("plusmn", '\u{b1}'),
    ("pound", '\u{a3}'),
    ("quot", '"'),
    ("raquo", '\u{bb}'),
    ("rarr", '\u{2192}'),
    ("rdquo", '\u{201d}'),
    ("reg", '\u{ae}'),
    ("rsaquo", '\u{203a}'),
    ("rsquo", '\u{2019}'),
    ("sect", '\u{a7}'),
    ("shy", '\u{ad}'),
    ("sup2", '\u{b2}'),
    ("szlig", '\u{df}'),
    ("times", '\u{d7}'),
    ("trade", '\u{2122}'),
    ("uuml", '\u{fc}'),
];

const MAX_ENTITY_NAME: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtractError {
    MissingMediaType,
    UnsupportedMediaType,
    Canonical(CanonicalError),
}

impl ExtractError {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::MissingMediaType => "missing_media_type",
            Self::UnsupportedMediaType => "unsupported_media_type",
            Self::Canonical(error) => error.code(),
        }
    }
}

impl std::fmt::Display for ExtractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for ExtractError {}

impl From<CanonicalError> for ExtractError {
    fn from(error: CanonicalError) -> Self {
        Self::Canonical(error)
    }
}

/// Canonical text and the media type essence it was extracted from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Extracted {
    pub media_type: String,
    pub text: String,
}

/// The media type essence and the declared charset of a Content-Type value.
///
/// # Errors
/// Refuses a malformed media type.
pub fn parse_content_type(value: &str) -> Result<(String, Option<String>), ExtractError> {
    let essence = canonical::media_type_essence(value)?;
    let charset = value.split(';').skip(1).find_map(|parameter| {
        let (name, value) = parameter.split_once('=')?;
        name.trim()
            .eq_ignore_ascii_case("charset")
            .then(|| value.trim().trim_matches('"').to_owned())
    });
    Ok((essence, charset))
}

/// Decodes, extracts and canonicalises a body by its Content-Type.
///
/// # Errors
/// Refuses a missing or unaccepted media type, an unsupported charset and
/// input invalid in the declared charset.
pub fn extract(content_type: Option<&str>, body: &[u8]) -> Result<Extracted, ExtractError> {
    let content_type = content_type
        .filter(|value| !value.trim().is_empty())
        .ok_or(ExtractError::MissingMediaType)?;
    let (media_type, charset) = parse_content_type(content_type)?;
    if !ACCEPTED_MEDIA_TYPES.contains(&media_type.as_str()) {
        return Err(ExtractError::UnsupportedMediaType);
    }
    let decoded = canonical::decode(body, charset.as_deref())?;
    let text = if media_type == "text/html" {
        html_text(&decoded)
    } else {
        decoded
    };
    Ok(Extracted {
        text: canonical::canonicalise(&text),
        media_type,
    })
}

enum Markup {
    Skip(usize),
    Tag {
        name: String,
        closing: bool,
        length: usize,
    },
    Text,
}

/// The text of an HTML document: script, style, noscript and template
/// elements dropped, block elements turned into line breaks and character
/// references decoded. Whitespace outside `pre` becomes spaces.
#[must_use]
pub fn html_text(source: &str) -> String {
    let mut text = String::with_capacity(source.len());
    let mut rest = source;
    let mut preformatted = 0_usize;
    while !rest.is_empty() {
        let Some(open) = rest.find('<') else {
            push_text(&mut text, rest, preformatted > 0);
            break;
        };
        push_text(&mut text, &rest[..open], preformatted > 0);
        rest = &rest[open..];
        match markup(rest) {
            Markup::Skip(length) => rest = &rest[length..],
            Markup::Text => {
                push_text(&mut text, "<", preformatted > 0);
                rest = &rest[1..];
            }
            Markup::Tag {
                name,
                closing,
                length,
            } => {
                rest = &rest[length..];
                if !closing && DROPPED.contains(&name.as_str()) {
                    rest = skip_element(rest, &name);
                    continue;
                }
                if name == "pre" {
                    preformatted = if closing {
                        preformatted.saturating_sub(1)
                    } else {
                        preformatted + 1
                    };
                }
                if BLOCKS.contains(&name.as_str()) {
                    text.push('\n');
                }
            }
        }
    }
    text
}

fn push_text(text: &mut String, raw: &str, preformatted: bool) {
    let start = text.len();
    decode_references(raw, text);
    if !preformatted {
        let tail: String = text[start..]
            .chars()
            .map(|character| {
                if matches!(character, '\t' | '\n' | '\r' | '\u{c}') {
                    ' '
                } else {
                    character
                }
            })
            .collect();
        text.truncate(start);
        text.push_str(&tail);
    }
}

fn tag_name(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .position(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':')))
        .unwrap_or(bytes.len())
}

fn markup(rest: &str) -> Markup {
    let bytes = rest.as_bytes();
    if let Some(comment) = rest.strip_prefix("<!--") {
        let length = comment.find("-->").map_or(rest.len(), |end| end + 7);
        return Markup::Skip(length);
    }
    if rest.starts_with("<!") || rest.starts_with("<?") {
        return Markup::Skip(rest.find('>').map_or(rest.len(), |end| end + 1));
    }
    let closing = rest.starts_with("</");
    let name_start = if closing { 2 } else { 1 };
    if !bytes.get(name_start).is_some_and(u8::is_ascii_alphabetic) {
        if closing {
            return Markup::Skip(rest.find('>').map_or(rest.len(), |end| end + 1));
        }
        return Markup::Text;
    }
    let name_length = tag_name(&bytes[name_start..]);
    let name = rest[name_start..name_start + name_length].to_ascii_lowercase();
    let mut quote = None;
    let mut length = rest.len();
    for (index, byte) in bytes.iter().enumerate().skip(name_start + name_length) {
        match (quote, byte) {
            (None, b'"' | b'\'') if !closing => quote = Some(*byte),
            (Some(open), _) if open == *byte => quote = None,
            (None, b'>') => {
                length = index + 1;
                break;
            }
            _ => {}
        }
    }
    Markup::Tag {
        name,
        closing,
        length,
    }
}

fn end_tag_at(rest: &str, name: &str) -> Option<usize> {
    let bytes = rest.as_bytes();
    let mut from = 0;
    while let Some(found) = rest[from..].find("</") {
        let start = from + found;
        let name_end = start + 2 + name.len();
        let named = bytes
            .get(start + 2..name_end)
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(name.as_bytes()));
        let delimited = bytes.get(name_end).is_none_or(|byte| {
            matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | b'\x0c' | b'/' | b'>')
        });
        if named && delimited {
            return Some(start);
        }
        from = start + 2;
    }
    None
}

fn skip_element<'a>(rest: &'a str, name: &str) -> &'a str {
    if RAW_TEXT.contains(&name) {
        return end_tag_at(rest, name).map_or("", |start| match markup(&rest[start..]) {
            Markup::Tag { length, .. } | Markup::Skip(length) => &rest[start + length..],
            Markup::Text => &rest[start + 1..],
        });
    }
    let mut depth = 1_usize;
    let mut remaining = rest;
    while let Some(open) = remaining.find('<') {
        remaining = &remaining[open..];
        match markup(remaining) {
            Markup::Skip(length) => remaining = &remaining[length..],
            Markup::Text => remaining = &remaining[1..],
            Markup::Tag {
                name: found,
                closing,
                length,
            } => {
                remaining = &remaining[length..];
                if !closing && RAW_TEXT.contains(&found.as_str()) {
                    remaining = skip_element(remaining, &found);
                } else if found == name {
                    if closing {
                        depth -= 1;
                        if depth == 0 {
                            return remaining;
                        }
                    } else {
                        depth += 1;
                    }
                }
            }
        }
    }
    ""
}

fn numeric_reference(digits: &str, radix: u32) -> char {
    let value = digits
        .chars()
        .try_fold(0_u32, |value, digit| {
            value
                .checked_mul(radix)?
                .checked_add(digit.to_digit(radix)?)
        })
        .filter(|value| *value != 0);
    value
        .and_then(char::from_u32)
        .unwrap_or(char::REPLACEMENT_CHARACTER)
}

fn decode_references(raw: &str, text: &mut String) {
    let mut rest = raw;
    while let Some(ampersand) = rest.find('&') {
        text.push_str(&rest[..ampersand]);
        rest = &rest[ampersand..];
        let body = &rest[1..];
        if let Some(number) = body.strip_prefix('#') {
            let (digits, radix, prefix) = match number.strip_prefix(['x', 'X']) {
                Some(hex) => (hex, 16, 2),
                None => (number, 10, 1),
            };
            let count = digits
                .find(|character: char| !character.is_digit(radix))
                .unwrap_or(digits.len());
            if count > 0 {
                text.push(numeric_reference(&digits[..count], radix));
                let semicolon = usize::from(digits[count..].starts_with(';'));
                rest = &rest[1 + prefix + count + semicolon..];
                continue;
            }
        } else {
            let count = body
                .find(|character: char| !character.is_ascii_alphanumeric())
                .unwrap_or(body.len());
            if count > 0 && count <= MAX_ENTITY_NAME && body[count..].starts_with(';') {
                if let Some((_, character)) =
                    ENTITIES.iter().find(|(name, _)| *name == &body[..count])
                {
                    text.push(*character);
                    rest = &rest[count + 2..];
                    continue;
                }
            }
        }
        text.push('&');
        rest = &rest[1..];
    }
    text.push_str(rest);
}
