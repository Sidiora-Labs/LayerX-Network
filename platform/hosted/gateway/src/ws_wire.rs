use std::io::{Read, Write};

const MAX_MESSAGE: usize = 64 * 1024;

pub(super) fn accept(key: &str) -> Option<String> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = key.as_bytes();
    if bytes.len() != 24
        || &bytes[22..] != b"=="
        || !bytes[..22].iter().all(|b| ALPHABET.contains(b))
        || ALPHABET.iter().position(|b| *b == bytes[21])? % 16 != 0
    {
        return None;
    }
    let mut input = key.as_bytes().to_vec();
    input.extend_from_slice(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    let bits = u64::try_from(input.len()).ok()?.checked_mul(8)?;
    input.push(128);
    while input.len() % 64 != 56 {
        input.push(0);
    }
    input.extend_from_slice(&bits.to_be_bytes());
    let mut state = [
        0x6745_2301_u32,
        0xefcd_ab89,
        0x98ba_dcfe,
        0x1032_5476,
        0xc3d2_e1f0,
    ];
    for block in input.chunks_exact(64) {
        let mut words = [0_u32; 80];
        for (word, bytes) in words.iter_mut().zip(block.chunks_exact(4)) {
            *word = u32::from_be_bytes(bytes.try_into().ok()?);
        }
        for i in 16..80 {
            words[i] = (words[i - 3] ^ words[i - 8] ^ words[i - 14] ^ words[i - 16]).rotate_left(1);
        }
        let [mut word_a, mut word_b, mut word_c, mut word_d, mut word_e] = state;
        for (i, word) in words.iter().enumerate() {
            let (round_value, round_constant) = match i {
                0..=19 => ((word_b & word_c) | (!word_b & word_d), 0x5a82_7999),
                20..=39 => (word_b ^ word_c ^ word_d, 0x6ed9_eba1),
                40..=59 => (
                    (word_b & word_c) | (word_b & word_d) | (word_c & word_d),
                    0x8f1b_bcdc,
                ),
                _ => (word_b ^ word_c ^ word_d, 0xca62_c1d6),
            };
            let next = word_a
                .rotate_left(5)
                .wrapping_add(round_value)
                .wrapping_add(word_e)
                .wrapping_add(round_constant)
                .wrapping_add(*word);
            word_e = word_d;
            word_d = word_c;
            word_c = word_b.rotate_left(30);
            word_b = word_a;
            word_a = next;
        }
        for (value, add) in state
            .iter_mut()
            .zip([word_a, word_b, word_c, word_d, word_e])
        {
            *value = value.wrapping_add(add);
        }
    }
    let digest: Vec<u8> = state.iter().flat_map(|word| word.to_be_bytes()).collect();
    let mut encoded = String::new();
    for bytes in digest.chunks(3) {
        let a = bytes[0];
        let b = bytes.get(1).copied().unwrap_or(0);
        let c = bytes.get(2).copied().unwrap_or(0);
        encoded.push(char::from(ALPHABET[usize::from(a >> 2)]));
        encoded.push(char::from(ALPHABET[usize::from(((a & 3) << 4) | (b >> 4))]));
        encoded.push(if bytes.len() > 1 {
            char::from(ALPHABET[usize::from(((b & 15) << 2) | (c >> 6))])
        } else {
            '='
        });
        encoded.push(if bytes.len() > 2 {
            char::from(ALPHABET[usize::from(c & 63)])
        } else {
            '='
        });
    }
    Some(encoded)
}

pub(super) fn write(stream: &mut impl Write, opcode: u8, body: &[u8]) -> Result<(), String> {
    let mut header = vec![0x80 | opcode];
    if let Some(length) = u8::try_from(body.len()).ok().filter(|length| *length < 126) {
        header.push(length);
    } else if let Ok(length) = u16::try_from(body.len()) {
        header.push(126);
        header.extend_from_slice(&length.to_be_bytes());
    } else {
        header.push(127);
        header.extend_from_slice(
            &u64::try_from(body.len())
                .map_err(|e| e.to_string())?
                .to_be_bytes(),
        );
    }
    stream
        .write_all(&header)
        .and_then(|()| stream.write_all(body))
        .and_then(|()| stream.flush())
        .map_err(|e| e.to_string())
}

#[derive(Default)]
pub(super) struct Reader {
    bytes: Vec<u8>,
    fragments: Option<Vec<u8>>,
}

impl Reader {
    pub(super) fn read(&mut self, stream: &mut impl Read) -> Result<Option<(u8, Vec<u8>)>, String> {
        if let Some(frame) = self.frame()? {
            return Ok(Some(frame));
        }
        let mut chunk = [0; 4096];
        match stream.read(&mut chunk) {
            Ok(0) => return Err("websocket disconnected".into()),
            Ok(n) => self.bytes.extend_from_slice(&chunk[..n]),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return Ok(None)
            }
            Err(e) => return Err(e.to_string()),
        }
        self.frame()
    }

    fn frame(&mut self) -> Result<Option<(u8, Vec<u8>)>, String> {
        if self.bytes.len() < 2 {
            return Ok(None);
        }
        let first = self.bytes[0];
        let opcode = first & 15;
        let short = self.bytes[1] & 127;
        if first & 0x70 != 0 || self.bytes[1] & 128 == 0 || !matches!(opcode, 0 | 1 | 8 | 9 | 10) {
            return Err("invalid websocket frame".into());
        }
        let extra = match short {
            126 => 2,
            127 => 8,
            _ => 0,
        };
        let header = 2 + extra + 4;
        if self.bytes.len() < header {
            return Ok(None);
        }
        let mut length = 0_usize;
        if extra == 0 {
            length = usize::from(short);
        } else {
            for byte in &self.bytes[2..2 + extra] {
                length = length
                    .checked_mul(256)
                    .and_then(|n| n.checked_add(usize::from(*byte)))
                    .ok_or("frame length overflow")?;
            }
            if (extra == 2 && length < 126) || (extra == 8 && length <= 65535) {
                return Err("noncanonical frame length".into());
            }
        }
        if length > MAX_MESSAGE || (opcode >= 8 && (length > 125 || first & 128 == 0)) {
            return Err("frame bound exceeded".into());
        }
        if self.bytes.len() < header + length {
            return Ok(None);
        }
        let mask = &self.bytes[header - 4..header];
        let body: Vec<_> = self.bytes[header..header + length]
            .iter()
            .enumerate()
            .map(|(i, b)| b ^ mask[i % 4])
            .collect();
        self.bytes.drain(..header + length);
        if opcode >= 8 {
            if opcode == 8
                && (body.len() == 1
                    || (body.len() >= 2
                        && (!valid_close(u16::from_be_bytes([body[0], body[1]]))
                            || std::str::from_utf8(&body[2..]).is_err())))
            {
                return Err("invalid close frame".into());
            }
            return Ok(Some((opcode, body)));
        }
        let mut message = match (opcode, self.fragments.take()) {
            (1, None) => Vec::new(),
            (0, Some(body)) => body,
            _ => return Err("invalid fragmentation".into()),
        };
        if message.len().saturating_add(body.len()) > MAX_MESSAGE {
            return Err("message bound exceeded".into());
        }
        message.extend_from_slice(&body);
        if first & 128 == 0 {
            self.fragments = Some(message);
            return Ok(None);
        }
        std::str::from_utf8(&message).map_err(|_| "invalid text message")?;
        Ok(Some((1, message)))
    }
}

fn valid_close(code: u16) -> bool {
    matches!(code, 1000..=1003 | 1007..=1014 | 3000..=4999)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handshake_vector_and_key_bounds() {
        assert_eq!(
            accept("dGhlIHNhbXBsZSBub25jZQ==").as_deref(),
            Some("s3pPLMBiTxaQ9kYGzzhZRbK+xOo=")
        );
        for key in [
            "",
            "dGhlIHNhbXBsZSBub25jZR==",
            "dGhlIHNhbXBsZSBub25jZQ=",
            "!!!!!!!!!!!!!!!!!!!!!!==",
        ] {
            assert!(accept(key).is_none());
        }
    }
    fn masked(first: u8, payload: &[u8]) -> Vec<u8> {
        let mut bytes = vec![
            first,
            128 | u8::try_from(payload.len()).unwrap_or(0),
            1,
            2,
            3,
            4,
        ];
        bytes.extend(
            payload
                .iter()
                .enumerate()
                .map(|(i, b)| b ^ [1, 2, 3, 4][i % 4]),
        );
        bytes
    }
    #[test]
    fn frames_enforce_mask_utf8_bounds_and_fragment_order() {
        for bytes in [
            vec![0x81, 0],
            vec![0xc1, 128, 0, 0, 0, 0],
            masked(0x80, b"orphan"),
            masked(0x81, &[255]),
            masked(0x88, &[1]),
            masked(0x88, &1005_u16.to_be_bytes()),
            vec![0x81, 255, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0],
        ] {
            assert!(Reader {
                bytes,
                fragments: None
            }
            .frame()
            .is_err());
        }
        let mut reader = Reader {
            bytes: masked(1, b"hel"),
            fragments: None,
        };
        assert_eq!(reader.frame(), Ok(None));
        reader.bytes.extend(masked(0x89, b"ping"));
        assert_eq!(reader.frame(), Ok(Some((9, b"ping".to_vec()))));
        reader.bytes.extend(masked(0x80, b"lo"));
        assert_eq!(reader.frame(), Ok(Some((1, b"hello".to_vec()))));
    }
}
