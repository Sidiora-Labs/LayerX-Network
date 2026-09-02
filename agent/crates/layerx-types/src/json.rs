//! Strict reader for repository-declared JSON configuration and vectors.

use std::fmt;

const MAXIMUM_DEPTH: usize = 64;

/// One parsed JSON value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JsonValue {
    /// The literal `null`.
    Null,
    /// A boolean literal.
    Bool(bool),
    /// A non-negative integer without fraction or exponent.
    Integer(u64),
    /// A string with escapes resolved.
    String(String),
    /// An ordered array.
    Array(Vec<JsonValue>),
    /// An object with unique keys in document order.
    Object(Vec<(String, JsonValue)>),
}

/// Exact JSON reading failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JsonError {
    /// The document violated JSON syntax or used an unsupported form.
    Syntax { offset: usize, detail: &'static str },
    /// One object repeated a key.
    DuplicateKey(String),
    /// A required path was absent.
    MissingField(String),
    /// A path held a value of another type.
    WrongType {
        path: String,
        expected: &'static str,
    },
    /// A hexadecimal string was malformed.
    Hex { path: String, detail: &'static str },
    /// A byte string had another exact length.
    Length {
        path: String,
        expected: usize,
        actual: usize,
    },
}

impl fmt::Display for JsonError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax { offset, detail } => {
                write!(formatter, "json syntax error at byte {offset}: {detail}")
            }
            Self::DuplicateKey(key) => write!(formatter, "json duplicate key {key}"),
            Self::MissingField(path) => write!(formatter, "json missing field {path}"),
            Self::WrongType { path, expected } => {
                write!(formatter, "json field {path} is not {expected}")
            }
            Self::Hex { path, detail } => write!(formatter, "json field {path}: {detail}"),
            Self::Length {
                path,
                expected,
                actual,
            } => write!(
                formatter,
                "json field {path} holds {actual} bytes, expected {expected}"
            ),
        }
    }
}

impl std::error::Error for JsonError {}

struct Parser<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl Parser<'_> {
    fn error(&self, detail: &'static str) -> JsonError {
        JsonError::Syntax {
            offset: self.offset,
            detail,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.offset).copied()
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.offset += 1;
        }
    }

    fn expect(&mut self, expected: u8, detail: &'static str) -> Result<(), JsonError> {
        if self.peek() == Some(expected) {
            self.offset += 1;
            Ok(())
        } else {
            Err(self.error(detail))
        }
    }

    fn literal(&mut self, word: &[u8], value: JsonValue) -> Result<JsonValue, JsonError> {
        if self.bytes[self.offset..].starts_with(word) {
            self.offset += word.len();
            Ok(value)
        } else {
            Err(self.error("invalid literal"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<JsonValue, JsonError> {
        if depth > MAXIMUM_DEPTH {
            return Err(self.error("nesting too deep"));
        }
        self.skip_whitespace();
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => self.string().map(JsonValue::String),
            Some(b'0'..=b'9') => self.integer(),
            Some(b't') => self.literal(b"true", JsonValue::Bool(true)),
            Some(b'f') => self.literal(b"false", JsonValue::Bool(false)),
            Some(b'n') => self.literal(b"null", JsonValue::Null),
            Some(b'-') => Err(self.error("negative numbers are not supported")),
            Some(_) => Err(self.error("unexpected character")),
            None => Err(self.error("unexpected end of document")),
        }
    }

    fn integer(&mut self) -> Result<JsonValue, JsonError> {
        let start = self.offset;
        let mut value: u64 = 0;
        while let Some(digit @ b'0'..=b'9') = self.peek() {
            if self.offset > start && self.bytes[start] == b'0' {
                return Err(self.error("leading zero"));
            }
            value = value
                .checked_mul(10)
                .and_then(|value| value.checked_add(u64::from(digit - b'0')))
                .ok_or_else(|| self.error("integer overflow"))?;
            self.offset += 1;
        }
        if matches!(self.peek(), Some(b'.' | b'e' | b'E')) {
            return Err(self.error("fractions and exponents are not supported"));
        }
        Ok(JsonValue::Integer(value))
    }

    fn hex_escape(&mut self) -> Result<u32, JsonError> {
        let digits = self
            .bytes
            .get(self.offset..self.offset + 4)
            .ok_or_else(|| self.error("truncated unicode escape"))?;
        let mut value = 0_u32;
        for digit in digits {
            let nibble = match digit {
                b'0'..=b'9' => digit - b'0',
                b'a'..=b'f' => digit - b'a' + 10,
                b'A'..=b'F' => digit - b'A' + 10,
                _ => return Err(self.error("invalid unicode escape")),
            };
            value = (value << 4) | u32::from(nibble);
        }
        self.offset += 4;
        Ok(value)
    }

    fn unicode_escape(&mut self, output: &mut String) -> Result<(), JsonError> {
        let first = self.hex_escape()?;
        let code_point = if (0xD800..0xDC00).contains(&first) {
            if self.bytes[self.offset..].starts_with(b"\\u") {
                self.offset += 2;
            } else {
                return Err(self.error("unpaired surrogate"));
            }
            let second = self.hex_escape()?;
            if !(0xDC00..0xE000).contains(&second) {
                return Err(self.error("unpaired surrogate"));
            }
            0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
        } else if (0xDC00..0xE000).contains(&first) {
            return Err(self.error("unpaired surrogate"));
        } else {
            first
        };
        let character =
            char::from_u32(code_point).ok_or_else(|| self.error("invalid unicode escape"))?;
        output.push(character);
        Ok(())
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.expect(b'"', "expected string")?;
        let mut output = String::new();
        loop {
            let byte = self
                .peek()
                .ok_or_else(|| self.error("unterminated string"))?;
            self.offset += 1;
            match byte {
                b'"' => return Ok(output),
                b'\\' => {
                    let escape = self
                        .peek()
                        .ok_or_else(|| self.error("unterminated escape"))?;
                    self.offset += 1;
                    match escape {
                        b'"' => output.push('"'),
                        b'\\' => output.push('\\'),
                        b'/' => output.push('/'),
                        b'b' => output.push('\u{8}'),
                        b'f' => output.push('\u{c}'),
                        b'n' => output.push('\n'),
                        b'r' => output.push('\r'),
                        b't' => output.push('\t'),
                        b'u' => self.unicode_escape(&mut output)?,
                        _ => return Err(self.error("invalid escape")),
                    }
                }
                0x00..=0x1f => return Err(self.error("control character in string")),
                _ => {
                    let start = self.offset - 1;
                    let mut end = self.offset;
                    while let Some(next) = self.bytes.get(end) {
                        if *next == b'"' || *next == b'\\' || *next < 0x20 {
                            break;
                        }
                        end += 1;
                    }
                    let text = std::str::from_utf8(&self.bytes[start..end])
                        .map_err(|_| self.error("invalid utf-8 in string"))?;
                    output.push_str(text);
                    self.offset = end;
                }
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<JsonValue, JsonError> {
        self.expect(b'[', "expected array")?;
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.offset += 1;
            return Ok(JsonValue::Array(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.offset += 1,
                Some(b']') => {
                    self.offset += 1;
                    return Ok(JsonValue::Array(items));
                }
                _ => return Err(self.error("expected comma or end of array")),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<JsonValue, JsonError> {
        self.expect(b'{', "expected object")?;
        let mut members: Vec<(String, JsonValue)> = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.offset += 1;
            return Ok(JsonValue::Object(members));
        }
        loop {
            self.skip_whitespace();
            let key = self.string()?;
            if members.iter().any(|(existing, _)| *existing == key) {
                return Err(JsonError::DuplicateKey(key));
            }
            self.skip_whitespace();
            self.expect(b':', "expected colon")?;
            let value = self.value(depth + 1)?;
            members.push((key, value));
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.offset += 1,
                Some(b'}') => {
                    self.offset += 1;
                    return Ok(JsonValue::Object(members));
                }
                _ => return Err(self.error("expected comma or end of object")),
            }
        }
    }
}

/// Parses one complete JSON document.
///
/// # Errors
///
/// Returns the first syntax violation, duplicate key, or unsupported number
/// form. Trailing content after the document is rejected.
pub fn parse(text: &str) -> Result<JsonValue, JsonError> {
    let mut parser = Parser {
        bytes: text.as_bytes(),
        offset: 0,
    };
    let value = parser.value(0)?;
    parser.skip_whitespace();
    if parser.offset != parser.bytes.len() {
        return Err(parser.error("trailing content after document"));
    }
    Ok(value)
}

impl JsonValue {
    /// Returns one object member by key.
    #[must_use]
    pub fn field(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Object(members) => members
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    /// Resolves a dotted path whose numeric segments index arrays.
    ///
    /// # Errors
    ///
    /// Returns a missing-field error naming the full path when any segment
    /// is absent.
    pub fn path(&self, path: &str) -> Result<&Self, JsonError> {
        let mut current = self;
        for segment in path.split('.') {
            current = match current {
                Self::Array(items) => segment
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| items.get(index)),
                Self::Object(_) => current.field(segment),
                _ => None,
            }
            .ok_or_else(|| JsonError::MissingField(path.to_owned()))?;
        }
        Ok(current)
    }

    /// Reads an unsigned integer at a dotted path.
    ///
    /// # Errors
    ///
    /// Returns a missing-field or wrong-type error.
    pub fn u64_at(&self, path: &str) -> Result<u64, JsonError> {
        match self.path(path)? {
            Self::Integer(value) => Ok(*value),
            _ => Err(JsonError::WrongType {
                path: path.to_owned(),
                expected: "an unsigned integer",
            }),
        }
    }

    /// Reads a boolean at a dotted path.
    ///
    /// # Errors
    ///
    /// Returns a missing-field or wrong-type error.
    pub fn bool_at(&self, path: &str) -> Result<bool, JsonError> {
        match self.path(path)? {
            Self::Bool(value) => Ok(*value),
            _ => Err(JsonError::WrongType {
                path: path.to_owned(),
                expected: "a boolean",
            }),
        }
    }

    /// Reads a string at a dotted path.
    ///
    /// # Errors
    ///
    /// Returns a missing-field or wrong-type error.
    pub fn str_at(&self, path: &str) -> Result<&str, JsonError> {
        match self.path(path)? {
            Self::String(value) => Ok(value),
            _ => Err(JsonError::WrongType {
                path: path.to_owned(),
                expected: "a string",
            }),
        }
    }

    /// Reads an array at a dotted path.
    ///
    /// # Errors
    ///
    /// Returns a missing-field or wrong-type error.
    pub fn array_at(&self, path: &str) -> Result<&[Self], JsonError> {
        match self.path(path)? {
            Self::Array(items) => Ok(items),
            _ => Err(JsonError::WrongType {
                path: path.to_owned(),
                expected: "an array",
            }),
        }
    }

    /// Reads the member names of an object at a dotted path.
    ///
    /// # Errors
    ///
    /// Returns a missing-field or wrong-type error.
    pub fn keys_at(&self, path: &str) -> Result<Vec<&str>, JsonError> {
        match self.path(path)? {
            Self::Object(members) => Ok(members.iter().map(|(key, _)| key.as_str()).collect()),
            _ => Err(JsonError::WrongType {
                path: path.to_owned(),
                expected: "an object",
            }),
        }
    }

    /// Reads a `0x`-prefixed lowercase-or-uppercase hexadecimal byte string.
    ///
    /// # Errors
    ///
    /// Returns a missing-field, wrong-type, or hex error.
    pub fn hex_at(&self, path: &str) -> Result<Vec<u8>, JsonError> {
        let text = self.str_at(path)?;
        decode_hex(text).map_err(|detail| JsonError::Hex {
            path: path.to_owned(),
            detail,
        })
    }

    /// Reads an exact-width hexadecimal byte string.
    ///
    /// # Errors
    ///
    /// Returns a missing-field, wrong-type, hex, or length error.
    pub fn hex_array_at<const WIDTH: usize>(&self, path: &str) -> Result<[u8; WIDTH], JsonError> {
        let bytes = self.hex_at(path)?;
        bytes.as_slice().try_into().map_err(|_| JsonError::Length {
            path: path.to_owned(),
            expected: WIDTH,
            actual: bytes.len(),
        })
    }
}

/// Decodes a `0x`-prefixed hexadecimal byte string.
///
/// # Errors
///
/// Returns a static description when the prefix, length, or digits are wrong.
pub fn decode_hex(text: &str) -> Result<Vec<u8>, &'static str> {
    let digits = text.strip_prefix("0x").ok_or("missing 0x prefix")?;
    if digits.len() % 2 != 0 {
        return Err("odd hexadecimal length");
    }
    digits
        .as_bytes()
        .chunks(2)
        .map(|pair| {
            let high = hex_nibble(pair[0])?;
            let low = hex_nibble(pair[1])?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn hex_nibble(value: u8) -> Result<u8, &'static str> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err("invalid hexadecimal digit"),
    }
}
