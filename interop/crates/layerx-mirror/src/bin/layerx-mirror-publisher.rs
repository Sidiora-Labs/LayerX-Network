use std::path::Path;

fn probe(address: &str) -> Result<(), ()> {
    use std::io::{Read as _, Write as _};
    use std::net::TcpStream;
    use std::time::Duration;

    let mut stream = TcpStream::connect(address).map_err(|_| ())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|_| ())?;
    stream
        .write_all(b"GET /readyz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .map_err(|_| ())?;
    let mut response = [0_u8; 64];
    let read = stream.read(&mut response).map_err(|_| ())?;
    if response[..read].starts_with(b"HTTP/1.1 200 ") {
        Ok(())
    } else {
        Err(())
    }
}

fn main() {
    let mut arguments = std::env::args_os();
    let _executable = arguments.next();
    let Some(config) = arguments.next() else {
        eprintln!("usage: layerx-mirror-publisher <config.json>");
        std::process::exit(64);
    };
    if config == "--probe" {
        let Some(address) = arguments.next() else {
            std::process::exit(64);
        };
        if arguments.next().is_some() {
            std::process::exit(64);
        }
        let address = address.to_string_lossy();
        std::process::exit(if probe(&address).is_ok() { 0 } else { 1 });
    }
    if arguments.next().is_some() {
        eprintln!("usage: layerx-mirror-publisher <config.json>");
        std::process::exit(64);
    }
    if let Err(error) = layerx_mirror::runtime::run(Path::new(&config)) {
        eprintln!("mirror publisher refused startup: {error:?}");
        std::process::exit(1);
    }
}
