const MAX_DEPTH: usize = 64;

/// One JSON value as exchanged with a Paxeer JSON-RPC endpoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(String),
    Text(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    #[must_use]
    pub fn member(&self, name: &str) -> Option<&Self> {
        match self {
            Self::Object(members) => members
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(value) => Some(value),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Self::Number(lexeme) => lexeme.parse().ok(),
            _ => None,
        }
    }

    #[must_use]
    pub const fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    #[must_use]
    pub fn render(&self) -> String {
        let mut output = String::new();
        self.render_into(&mut output);
        output
    }

    fn render_into(&self, output: &mut String) {
        match self {
            Self::Null => output.push_str("null"),
            Self::Bool(true) => output.push_str("true"),
            Self::Bool(false) => output.push_str("false"),
            Self::Number(lexeme) => output.push_str(lexeme),
            Self::Text(text) => render_text(text, output),
            Self::Array(items) => {
                output.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        output.push(',');
                    }
                    item.render_into(output);
                }
                output.push(']');
            }
            Self::Object(members) => {
                output.push('{');
                for (index, (key, value)) in members.iter().enumerate() {
                    if index > 0 {
                        output.push(',');
                    }
                    render_text(key, output);
                    output.push(':');
                    value.render_into(output);
                }
                output.push('}');
            }
        }
    }
}

fn render_text(text: &str, output: &mut String) {
    output.push('"');
    for character in text.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            control if u32::from(control) < 0x20 => {
                let code = u32::from(control);
                output.push_str("\\u");
                for shift in [12_u32, 8, 4, 0] {
                    let nibble = (code >> shift) & 0x0f;
                    output.push(hex_digit(nibble));
                }
            }
            other => output.push(other),
        }
    }
    output.push('"');
}

fn hex_digit(nibble: u32) -> char {
    char::from_digit(nibble, 16).unwrap_or('0')
}

/// Why a response body failed to parse as JSON.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JsonErrorReason {
    UnexpectedEnd,
    UnexpectedByte(u8),
    InvalidNumber,
    InvalidEscape,
    InvalidUnicode,
    InvalidUtf8,
    DepthExceeded,
    DuplicateMember,
    TrailingData,
}

/// Parse defect with the byte offset it was detected at.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JsonError {
    pub offset: usize,
    pub reason: JsonErrorReason,
}

/// Parses one complete JSON document.
///
/// # Errors
///
/// Returns the first grammar, depth or encoding defect encountered.
pub fn parse(text: &str) -> Result<Json, JsonError> {
    let mut parser = Parser {
        bytes: text.as_bytes(),
        offset: 0,
    };
    parser.skip_whitespace();
    let value = parser.value(0)?;
    parser.skip_whitespace();
    if parser.offset == parser.bytes.len() {
        Ok(value)
    } else {
        Err(parser.defect(JsonErrorReason::TrailingData))
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl Parser<'_> {
    const fn defect(&self, reason: JsonErrorReason) -> JsonError {
        JsonError {
            offset: self.offset,
            reason,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.offset).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.offset = self.offset.saturating_add(1);
        Some(byte)
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.offset = self.offset.saturating_add(1);
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, JsonError> {
        if depth >= MAX_DEPTH {
            return Err(self.defect(JsonErrorReason::DepthExceeded));
        }
        self.skip_whitespace();
        match self.peek() {
            None => Err(self.defect(JsonErrorReason::UnexpectedEnd)),
            Some(b'n') => self.literal(b"null", Json::Null),
            Some(b't') => self.literal(b"true", Json::Bool(true)),
            Some(b'f') => self.literal(b"false", Json::Bool(false)),
            Some(b'"') => self.text().map(Json::Text),
            Some(b'[') => self.array(depth),
            Some(b'{') => self.object(depth),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(byte) => Err(self.defect(JsonErrorReason::UnexpectedByte(byte))),
        }
    }

    fn literal(&mut self, word: &[u8], value: Json) -> Result<Json, JsonError> {
        let end = self.offset.saturating_add(word.len());
        if self.bytes.get(self.offset..end) == Some(word) {
            self.offset = end;
            Ok(value)
        } else {
            match self.peek() {
                Some(byte) => Err(self.defect(JsonErrorReason::UnexpectedByte(byte))),
                None => Err(self.defect(JsonErrorReason::UnexpectedEnd)),
            }
        }
    }

    fn digits(&mut self) -> usize {
        let mut count = 0_usize;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.offset = self.offset.saturating_add(1);
            count = count.saturating_add(1);
        }
        count
    }

    fn number(&mut self) -> Result<Json, JsonError> {
        let start = self.offset;
        if self.peek() == Some(b'-') {
            self.offset = self.offset.saturating_add(1);
        }
        if self.digits() == 0 {
            return Err(self.defect(JsonErrorReason::InvalidNumber));
        }
        if self.peek() == Some(b'.') {
            self.offset = self.offset.saturating_add(1);
            if self.digits() == 0 {
                return Err(self.defect(JsonErrorReason::InvalidNumber));
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.offset = self.offset.saturating_add(1);
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.offset = self.offset.saturating_add(1);
            }
            if self.digits() == 0 {
                return Err(self.defect(JsonErrorReason::InvalidNumber));
            }
        }
        let lexeme = self
            .bytes
            .get(start..self.offset)
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .ok_or_else(|| self.defect(JsonErrorReason::InvalidNumber))?;
        Ok(Json::Number(lexeme.to_owned()))
    }

    fn text(&mut self) -> Result<String, JsonError> {
        match self.bump() {
            Some(b'"') => {}
            Some(byte) => return Err(self.defect(JsonErrorReason::UnexpectedByte(byte))),
            None => return Err(self.defect(JsonErrorReason::UnexpectedEnd)),
        }
        let mut buffer = Vec::new();
        loop {
            match self.bump() {
                None => return Err(self.defect(JsonErrorReason::UnexpectedEnd)),
                Some(b'"') => {
                    return String::from_utf8(buffer)
                        .map_err(|_| self.defect(JsonErrorReason::InvalidUtf8));
                }
                Some(b'\\') => self.escape(&mut buffer)?,
                Some(byte) if byte < 0x20 => {
                    return Err(self.defect(JsonErrorReason::UnexpectedByte(byte)));
                }
                Some(byte) => buffer.push(byte),
            }
        }
    }

    fn escape(&mut self, buffer: &mut Vec<u8>) -> Result<(), JsonError> {
        match self.bump() {
            None => Err(self.defect(JsonErrorReason::UnexpectedEnd)),
            Some(b'"') => {
                buffer.push(b'"');
                Ok(())
            }
            Some(b'\\') => {
                buffer.push(b'\\');
                Ok(())
            }
            Some(b'/') => {
                buffer.push(b'/');
                Ok(())
            }
            Some(b'b') => {
                buffer.push(0x08);
                Ok(())
            }
            Some(b'f') => {
                buffer.push(0x0c);
                Ok(())
            }
            Some(b'n') => {
                buffer.push(b'\n');
                Ok(())
            }
            Some(b'r') => {
                buffer.push(b'\r');
                Ok(())
            }
            Some(b't') => {
                buffer.push(b'\t');
                Ok(())
            }
            Some(b'u') => self.unicode_escape(buffer),
            Some(_) => Err(self.defect(JsonErrorReason::InvalidEscape)),
        }
    }

    fn unicode_escape(&mut self, buffer: &mut Vec<u8>) -> Result<(), JsonError> {
        let first = self.hex4()?;
        let code = if (0xd800..=0xdbff).contains(&first) {
            if self.bump() != Some(b'\\') || self.bump() != Some(b'u') {
                return Err(self.defect(JsonErrorReason::InvalidUnicode));
            }
            let second = self.hex4()?;
            if !(0xdc00..=0xdfff).contains(&second) {
                return Err(self.defect(JsonErrorReason::InvalidUnicode));
            }
            0x10000_u32
                .saturating_add((first.saturating_sub(0xd800)) << 10)
                .saturating_add(second.saturating_sub(0xdc00))
        } else {
            first
        };
        let character =
            char::from_u32(code).ok_or_else(|| self.defect(JsonErrorReason::InvalidUnicode))?;
        let mut encoded = [0_u8; 4];
        buffer.extend_from_slice(character.encode_utf8(&mut encoded).as_bytes());
        Ok(())
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        let mut value = 0_u32;
        for _ in 0..4 {
            let digit = match self.bump() {
                Some(byte @ b'0'..=b'9') => byte - b'0',
                Some(byte @ b'a'..=b'f') => byte - b'a' + 10,
                Some(byte @ b'A'..=b'F') => byte - b'A' + 10,
                Some(_) => return Err(self.defect(JsonErrorReason::InvalidEscape)),
                None => return Err(self.defect(JsonErrorReason::UnexpectedEnd)),
            };
            value = (value << 4) | u32::from(digit);
        }
        Ok(value)
    }

    fn array(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.offset = self.offset.saturating_add(1);
        self.skip_whitespace();
        let mut items = Vec::new();
        if self.peek() == Some(b']') {
            self.offset = self.offset.saturating_add(1);
            return Ok(Json::Array(items));
        }
        loop {
            items.push(self.value(depth.saturating_add(1))?);
            self.skip_whitespace();
            match self.bump() {
                Some(b',') => {}
                Some(b']') => return Ok(Json::Array(items)),
                Some(byte) => return Err(self.defect(JsonErrorReason::UnexpectedByte(byte))),
                None => return Err(self.defect(JsonErrorReason::UnexpectedEnd)),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.offset = self.offset.saturating_add(1);
        self.skip_whitespace();
        let mut members = Vec::new();
        if self.peek() == Some(b'}') {
            self.offset = self.offset.saturating_add(1);
            return Ok(Json::Object(members));
        }
        loop {
            self.skip_whitespace();
            let key = self.text()?;
            if members.iter().any(|(existing, _)| existing == &key) {
                return Err(self.defect(JsonErrorReason::DuplicateMember));
            }
            self.skip_whitespace();
            match self.bump() {
                Some(b':') => {}
                Some(byte) => return Err(self.defect(JsonErrorReason::UnexpectedByte(byte))),
                None => return Err(self.defect(JsonErrorReason::UnexpectedEnd)),
            }
            members.push((key, self.value(depth.saturating_add(1))?));
            self.skip_whitespace();
            match self.bump() {
                Some(b',') => {}
                Some(b'}') => return Ok(Json::Object(members)),
                Some(byte) => return Err(self.defect(JsonErrorReason::UnexpectedByte(byte))),
                None => return Err(self.defect(JsonErrorReason::UnexpectedEnd)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_object_members_are_ambiguous() {
        let error = parse(r#"{"result":"0x01","result":"0x02"}"#)
            .err()
            .unwrap_or_else(|| panic!("duplicate JSON member was accepted"));

        assert_eq!(error.reason, JsonErrorReason::DuplicateMember);
    }
}
