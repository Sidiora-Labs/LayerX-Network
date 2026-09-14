use layerx_types::clock::{Clock, Deadline};
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use crate::{invalid, State};

const MAX_FRAME: usize = 1_048_576;
const MAX_BINDING_FRAME: usize = 2048;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingRequest {
    operation: String,
    tenant: String,
    principal: String,
}

pub(crate) fn serve_binding(
    stream: &mut UnixStream,
    state: &State,
    deadline: Duration,
    clock: &dyn Clock,
) -> io::Result<()> {
    let mut expires = Deadline::start(clock, deadline).map_err(io::Error::other)?;
    let request = read_binding(stream, &mut expires, clock);
    state.ready()?;
    let response = match request.and_then(|request| {
        let did = state.principal_binding(&request.tenant, &request.principal)?;
        Ok(serde_json::json!({"status":"bound", "tenant":request.tenant, "principal":request.principal, "did":did}))
    }) {
        Ok(response) => response,
        Err(_) => serde_json::json!({"status":"refused"}),
    };
    let mut bytes = b"LXIB\x01".to_vec();
    bytes.extend(serde_json::to_vec(&response)?);
    if bytes.len() > MAX_BINDING_FRAME {
        return Err(invalid("binding response exceeds bound"));
    }
    let mut frame = u32::try_from(bytes.len())
        .map_err(|_| invalid("binding length"))?
        .to_be_bytes()
        .to_vec();
    frame.extend(bytes);
    let _ = write_before(stream, &frame, &mut expires, clock);
    Ok(())
}

fn read_binding(
    stream: &mut UnixStream,
    expires: &mut Deadline,
    clock: &dyn Clock,
) -> io::Result<BindingRequest> {
    let mut length = [0; 4];
    read_before(stream, &mut length, expires, clock)?;
    let length = u32::from_be_bytes(length) as usize;
    if !(6..=MAX_BINDING_FRAME).contains(&length) {
        return Err(invalid("binding frame length"));
    }
    let mut bytes = vec![0; length];
    read_before(stream, &mut bytes, expires, clock)?;
    if &bytes[..5] != b"LXIB\x01" {
        return Err(invalid("binding frame version"));
    }
    let request: BindingRequest =
        serde_json::from_slice(&bytes[5..]).map_err(|_| invalid("binding request"))?;
    if request.operation != "principal"
        || request.tenant.is_empty()
        || request.tenant.len() > 255
        || request.tenant.chars().any(char::is_control)
        || request.principal.is_empty()
        || request.principal.len() > 255
    {
        return Err(invalid("binding request"));
    }
    Ok(request)
}

pub(crate) fn serve(
    stream: &mut UnixStream,
    state: &mut State,
    deadline: Duration,
    clock: &dyn Clock,
) -> io::Result<()> {
    let mut expires = Deadline::start(clock, deadline).map_err(io::Error::other)?;
    let request = read_request(stream, &mut expires, clock);
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
    let _ = write_before(stream, &framed, &mut expires, clock);
    Ok(())
}

fn read_request(
    stream: &mut UnixStream,
    expires: &mut Deadline,
    clock: &dyn Clock,
) -> io::Result<(u8, Vec<Vec<u8>>)> {
    let mut length = [0; 4];
    read_before(stream, &mut length, expires, clock)?;
    let length = u32::from_be_bytes(length) as usize;
    if !(10..=MAX_FRAME).contains(&length) {
        return Err(invalid("frame length"));
    }
    let mut bytes = vec![0; length];
    read_before(stream, &mut bytes, expires, clock)?;
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

fn remaining(expires: &mut Deadline, clock: &dyn Clock) -> io::Result<Duration> {
    let remaining = expires.remaining(clock).map_err(io::Error::other)?;
    if remaining.is_zero() {
        return Err(io::Error::new(io::ErrorKind::TimedOut, "frame deadline"));
    }
    Ok(remaining)
}

fn read_before(
    stream: &mut UnixStream,
    mut bytes: &mut [u8],
    expires: &mut Deadline,
    clock: &dyn Clock,
) -> io::Result<()> {
    while !bytes.is_empty() {
        stream.set_read_timeout(Some(remaining(expires, clock)?))?;
        match stream.read(bytes) {
            Ok(0) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
            Ok(count) => bytes = &mut bytes[count..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn write_before(
    stream: &mut UnixStream,
    mut bytes: &[u8],
    expires: &mut Deadline,
    clock: &dyn Clock,
) -> io::Result<()> {
    while !bytes.is_empty() {
        stream.set_write_timeout(Some(remaining(expires, clock)?))?;
        match stream.write(bytes) {
            Ok(0) => return Err(io::Error::from(io::ErrorKind::WriteZero)),
            Ok(count) => bytes = &bytes[count..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
