use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use crate::{invalid, State};

const MAX_FRAME: usize = 1_048_576;

pub(crate) fn serve(
    stream: &mut UnixStream,
    state: &mut State,
    deadline: Duration,
) -> io::Result<()> {
    let expires = Instant::now() + deadline;
    let request = read_request(stream, expires);
    state.ready()?;
    let result = match request {
        Ok((0, fields)) if fields.is_empty() => Ok(Vec::new()),
        Ok((1, fields)) if fields.len() == 4 => state.provision(&fields),
        Ok((2, fields)) if fields.len() == 1 => state.resolve(&fields),
        Ok((3, fields)) if fields.len() == 2 => state.device(&fields),
        _ => Err(invalid("invalid request")),
    };
    let (status, fields) = match result {
        Ok(fields) => (0, fields),
        Err(error) if error.kind() == io::ErrorKind::InvalidData => (1, Vec::new()),
        Err(error) => return Err(error),
    };
    let mut response = b"LXIP\x01".to_vec();
    response.push(status);
    response.extend_from_slice(
        &u32::try_from(fields.len())
            .map_err(|_| invalid("field count"))?
            .to_be_bytes(),
    );
    for field in fields {
        response.extend_from_slice(
            &u32::try_from(field.len())
                .map_err(|_| invalid("field length"))?
                .to_be_bytes(),
        );
        response.extend_from_slice(&field);
    }
    let mut framed = u32::try_from(response.len())
        .map_err(|_| invalid("response length"))?
        .to_be_bytes()
        .to_vec();
    framed.extend_from_slice(&response);
    let _ = write_before(stream, &framed, expires);
    Ok(())
}

fn read_request(stream: &mut UnixStream, expires: Instant) -> io::Result<(u8, Vec<Vec<u8>>)> {
    let mut length = [0; 4];
    read_before(stream, &mut length, expires)?;
    let length = u32::from_be_bytes(length) as usize;
    if !(10..=MAX_FRAME).contains(&length) {
        return Err(invalid("frame length"));
    }
    let mut bytes = vec![0; length];
    read_before(stream, &mut bytes, expires)?;
    decode(&bytes)
}

fn decode(bytes: &[u8]) -> io::Result<(u8, Vec<Vec<u8>>)> {
    if bytes.len() < 10 || &bytes[..5] != b"LXIP\x01" {
        return Err(invalid("frame header"));
    }
    let operation = bytes[5];
    let expected = match operation {
        0 => 0,
        1 => 4,
        2 => 1,
        3 => 2,
        _ => return Err(invalid("operation")),
    };
    let count = u32::from_be_bytes(
        bytes[6..10]
            .try_into()
            .map_err(|_| invalid("field count"))?,
    );
    if count != expected {
        return Err(invalid("field count"));
    }
    let mut fields = Vec::new();
    let mut remaining = &bytes[10..];
    for _ in 0..count {
        let prefix = remaining.get(..4).ok_or_else(|| invalid("field prefix"))?;
        let length =
            u32::from_be_bytes(prefix.try_into().map_err(|_| invalid("field prefix"))?) as usize;
        remaining = &remaining[4..];
        let field = remaining
            .get(..length)
            .ok_or_else(|| invalid("field length"))?;
        fields.push(field.to_vec());
        remaining = &remaining[length..];
    }
    if !remaining.is_empty() {
        return Err(invalid("trailing bytes"));
    }
    Ok((operation, fields))
}

fn remaining(expires: Instant) -> io::Result<Duration> {
    expires
        .checked_duration_since(Instant::now())
        .filter(|value| !value.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "frame deadline"))
}

fn read_before(stream: &mut UnixStream, mut bytes: &mut [u8], expires: Instant) -> io::Result<()> {
    while !bytes.is_empty() {
        stream.set_read_timeout(Some(remaining(expires)?))?;
        match stream.read(bytes) {
            Ok(0) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
            Ok(count) => bytes = &mut bytes[count..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn write_before(stream: &mut UnixStream, mut bytes: &[u8], expires: Instant) -> io::Result<()> {
    while !bytes.is_empty() {
        stream.set_write_timeout(Some(remaining(expires)?))?;
        match stream.write(bytes) {
            Ok(0) => return Err(io::Error::from(io::ErrorKind::WriteZero)),
            Ok(count) => bytes = &bytes[count..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
