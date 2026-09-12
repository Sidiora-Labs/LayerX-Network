use super::{
    config, connect_human_authority, connect_human_node, human_lni_limits, human_peers, optional,
    parse_u64, required, response, serve, start_human_owner, HEADER_LIMIT,
};
use std::env;
use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

#[derive(Debug, PartialEq, Eq)]
enum Mode {
    Full,
    HumanOwner,
}

impl Mode {
    fn parse(value: Option<&str>) -> Result<Self, String> {
        match value {
            None | Some("full") => Ok(Self::Full),
            Some("human-owner") => Ok(Self::HumanOwner),
            Some(_) => Err("LAYERX_AGENT_MODE is invalid".to_owned()),
        }
    }
}

pub(super) fn run() -> Result<(), String> {
    let mode = match env::var("LAYERX_AGENT_MODE") {
        Ok(value) => Mode::parse(Some(&value))?,
        Err(env::VarError::NotPresent) => Mode::parse(None)?,
        Err(env::VarError::NotUnicode(_)) => return Err("LAYERX_AGENT_MODE is invalid".to_owned()),
    };
    match mode {
        Mode::Full => config().and_then(serve),
        Mode::HumanOwner => serve_human_owner(),
    }
}

fn dependencies_ready() -> Result<(), String> {
    let deadline = Duration::from_millis(parse_u64("LAYERX_AGENT_HUMAN_DEADLINE_MS")?);
    let human_limits = human_lni_limits(deadline)?;
    let node_limits = layerx_client::lni::transport::Limits {
        maximum_frame_bytes: human_limits
            .maximum_frame_bytes
            .max(layerx_client::evidence::MINIMUM_FINALITY_FRAME_BYTES),
        ..human_limits
    };
    connect_human_node(
        PathBuf::from(required("LAYERX_AGENT_HUMAN_NODE_LNI")?),
        node_limits,
    )?;
    connect_human_authority(deadline, &human_peers()?)?;
    Ok(())
}

fn owner_running(human: &mpsc::Receiver<Result<(), String>>) -> Result<(), String> {
    match human.try_recv() {
        Err(mpsc::TryRecvError::Empty) => Ok(()),
        Ok(Err(error)) => Err(error),
        Ok(Ok(())) | Err(mpsc::TryRecvError::Disconnected) => {
            Err("human listener terminated".to_owned())
        }
    }
}

fn serve_human_owner() -> Result<(), String> {
    let listen = required("LAYERX_AGENT_PROGRAM_LISTEN")?;
    let bearer = required("LAYERX_AGENT_PROGRAM_BEARER_TOKEN")?;
    if !listen.starts_with("127.0.0.1:") || bearer.len() < 32 {
        return Err("human health requires loopback and a bounded credential".to_owned());
    }
    if bearer == required("LAYERX_AGENT_HUMAN_AUTHORITY_BEARER")? {
        return Err("human health and authority credentials must be distinct".to_owned());
    }
    if optional("LAYERX_AGENT_MCP_BINDING_ROOT").is_some() {
        return Err(
            "LAYERX_AGENT_MCP_BINDING_ROOT requires the full agent mode, which serves the program endpoint and probe program the binding names"
                .to_owned(),
        );
    }
    let human = start_human_owner(None)?;
    let listener = TcpListener::bind(listen)
        .map_err(|error| format!("human health listener failed: {error}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("human health nonblocking setup failed: {error}"))?;
    loop {
        owner_running(&human)?;
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(25));
                continue;
            }
            Err(error) => return Err(format!("human health accept failed: {error}")),
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .and_then(|()| stream.set_write_timeout(Some(Duration::from_secs(10))))
            .map_err(|error| format!("human health connection timeout setup failed: {error}"))?;
        let _ = serve_health(&mut stream, &bearer, &human);
    }
}

fn serve_health(
    stream: &mut TcpStream,
    bearer: &str,
    human: &mpsc::Receiver<Result<(), String>>,
) -> Result<(), String> {
    let mut bytes = [0_u8; HEADER_LIMIT];
    let mut length = 0;
    while length < bytes.len() && !bytes[..length].windows(4).any(|value| value == b"\r\n\r\n") {
        let count = stream
            .read(&mut bytes[length..])
            .map_err(|error| format!("agent request failed: {error}"))?;
        if count == 0 {
            return Err("agent request ended before its headers".to_owned());
        }
        length += count;
    }
    if !bytes[..length].windows(4).any(|value| value == b"\r\n\r\n") {
        return response(stream, 431, "{\"error\":\"headers_too_large\"}");
    }
    let request = std::str::from_utf8(&bytes[..length])
        .map_err(|_| "agent request headers are not UTF-8".to_owned())?;
    let line = request.lines().next().unwrap_or_default();
    let mut parts = line.split_ascii_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();
    if parts.next() != Some("HTTP/1.1") || parts.next().is_some() || method != "GET" {
        return response(stream, 400, "{\"error\":\"invalid_request\"}");
    }
    if !request
        .lines()
        .any(|header| header.strip_prefix("Authorization: Bearer ") == Some(bearer))
    {
        return response(stream, 401, "{\"error\":\"unauthorized\"}");
    }
    if path != "/healthz" {
        return response(stream, 404, "{\"error\":\"not_found\"}");
    }
    let ready = owner_running(human)
        .and_then(|()| dependencies_ready())
        .and_then(|()| owner_running(human))
        .is_ok();
    if ready {
        response(stream, 200, "{\"ready\":true}")
    } else {
        response(stream, 503, "{\"ready\":false}")
    }
}

#[cfg(test)]
mod tests {
    use super::Mode;

    #[test]
    fn mode_selection_is_explicit_and_closed() {
        assert_eq!(Mode::parse(None), Ok(Mode::Full));
        assert_eq!(Mode::parse(Some("full")), Ok(Mode::Full));
        assert_eq!(Mode::parse(Some("human-owner")), Ok(Mode::HumanOwner));
        for value in ["", "human", "FULL", "human-owner "] {
            assert!(Mode::parse(Some(value)).is_err());
        }
    }

    #[test]
    fn health_refuses_terminated_owner_and_preserves_http_boundaries(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        use std::sync::mpsc;
        use std::thread;
        use std::time::Duration;

        let bearer = "h".repeat(32);
        let cases = [
            (format!("GET /healthz HTTP/1.1\r\nAuthorization: Bearer {bearer}\r\n\r\n"), 503, "{\"ready\":false}"),
            ("GET /healthz HTTP/1.1\r\n\r\n".to_owned(), 401, "unauthorized"),
            (format!("GET /v1/programs/anything/balances HTTP/1.1\r\nAuthorization: Bearer {bearer}\r\n\r\n"), 404, "not_found"),
            ("POST /healthz HTTP/1.1\r\n\r\n".to_owned(), 400, "invalid_request"),
            ("x".repeat(super::HEADER_LIMIT), 431, "headers_too_large"),
        ];
        for (request, status, body) in cases {
            let listener = TcpListener::bind("127.0.0.1:0")?;
            let address = listener.local_addr()?;
            let bearer = bearer.clone();
            let server = thread::spawn(move || -> Result<(), String> {
                let (mut stream, _) = listener.accept().map_err(|e| e.to_string())?;
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .map_err(|e| e.to_string())?;
                let (sender, receiver) = mpsc::channel();
                drop(sender);
                super::serve_health(&mut stream, &bearer, &receiver)
            });
            let mut client = TcpStream::connect(address)?;
            client.set_read_timeout(Some(Duration::from_secs(5)))?;
            client.write_all(request.as_bytes())?;
            let mut response = String::new();
            client.read_to_string(&mut response)?;
            server.join().map_err(|_| "health server panicked")??;
            assert!(response.starts_with(&format!("HTTP/1.1 {status} ")));
            assert!(response.contains(body));
        }
        Ok(())
    }
}
