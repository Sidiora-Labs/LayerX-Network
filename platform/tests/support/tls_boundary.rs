use std::fs;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct Boundary(Child);

impl Drop for Boundary {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn openssl(directory: &Path, arguments: &[&str]) {
    let output = Command::new("openssl")
        .args(arguments)
        .current_dir(directory)
        .output()
        .unwrap_or_else(|error| panic!("start OpenSSL: {error}"));
    assert!(
        output.status.success(),
        "OpenSSL qualification prerequisite"
    );
}

fn certificates(directory: &Path) {
    openssl(
        directory,
        &[
            "req",
            "-x509",
            "-newkey",
            "ec",
            "-pkeyopt",
            "ec_paramgen_curve:P-256",
            "-nodes",
            "-keyout",
            "key.pem",
            "-out",
            "cert.pem",
            "-days",
            "1",
            "-subj",
            "/CN=localhost",
            "-addext",
            "subjectAltName=DNS:localhost",
        ],
    );
    fs::rename(directory.join("key.pem"), directory.join("ca-key.pem"))
        .unwrap_or_else(|error| panic!("retain root signing key: {error}"));
    openssl(
        directory,
        &[
            "req",
            "-new",
            "-newkey",
            "ec",
            "-pkeyopt",
            "ec_paramgen_curve:P-256",
            "-nodes",
            "-keyout",
            "key.pem",
            "-out",
            "server.csr",
            "-subj",
            "/CN=localhost",
        ],
    );
    fs::write(directory.join("server.ext"), "basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth\nsubjectAltName=DNS:localhost\n")
        .unwrap_or_else(|error| panic!("server certificate extensions: {error}"));
    openssl(
        directory,
        &[
            "x509",
            "-req",
            "-in",
            "server.csr",
            "-CA",
            "cert.pem",
            "-CAkey",
            "ca-key.pem",
            "-CAcreateserial",
            "-out",
            "server.pem",
            "-days",
            "1",
            "-extfile",
            "server.ext",
        ],
    );
    openssl(
        directory,
        &[
            "x509",
            "-in",
            "server.pem",
            "-outform",
            "DER",
            "-out",
            "server.der",
        ],
    );
    openssl(
        directory,
        &[
            "pkcs8",
            "-topk8",
            "-nocrypt",
            "-in",
            "ca-key.pem",
            "-outform",
            "DER",
            "-out",
            "ca-key.der",
        ],
    );
    openssl(
        directory,
        &[
            "req",
            "-x509",
            "-newkey",
            "ec",
            "-pkeyopt",
            "ec_paramgen_curve:P-256",
            "-nodes",
            "-keyout",
            "other-key.pem",
            "-out",
            "other-cert.pem",
            "-days",
            "1",
            "-subj",
            "/CN=unrelated-root",
        ],
    );
    openssl(
        directory,
        &[
            "x509",
            "-in",
            "other-cert.pem",
            "-outform",
            "DER",
            "-out",
            "other-cert.der",
        ],
    );
    openssl(
        directory,
        &[
            "x509", "-in", "cert.pem", "-outform", "DER", "-out", "cert.der",
        ],
    );
    openssl(
        directory,
        &[
            "pkcs8", "-topk8", "-nocrypt", "-in", "key.pem", "-outform", "DER", "-out", "key.der",
        ],
    );
}

fn start(directory: &Path, certificate: &str, key: &str) -> (Boundary, u16) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .unwrap_or_else(|error| panic!("reserve boundary port: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("boundary address: {error}"));
    drop(listener);
    let executable = std::env::var_os("LAYERX_PAXEER_BOUNDARY_BIN").map_or_else(
        || {
            std::env::current_exe()
                .unwrap_or_else(|error| panic!("test executable: {error}"))
                .parent()
                .and_then(Path::parent)
                .unwrap_or_else(|| panic!("Cargo target directory"))
                .join("layerx-paxeer-boundary")
        },
        PathBuf::from,
    );
    assert!(
        executable.is_file(),
        "build the real Paxeer TLS boundary first"
    );
    let mut command = Command::new(executable);
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("LAYERX_PAXEER_") {
            command.env_remove(name);
        }
    }
    let process = command
        .env("LAYERX_PAXEER_CHAIN_ID", "125")
        .env("LAYERX_PAXEER_BOUNDARY_LISTEN", address.to_string())
        .env("LAYERX_PAXEER_NODE_URL", "http://127.0.0.1:1")
        .env(
            "LAYERX_PAXEER_BOUNDARY_TLS_CERT_DER",
            directory.join(certificate),
        )
        .env("LAYERX_PAXEER_BOUNDARY_TLS_KEY_DER", directory.join(key))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|error| panic!("start real TLS boundary: {error}"));
    let mut boundary = Boundary(process);
    for _ in 0..100 {
        assert!(boundary
            .0
            .try_wait()
            .unwrap_or_else(|error| panic!("boundary status: {error}"))
            .is_none());
        if TcpStream::connect_timeout(&address, Duration::from_millis(50)).is_ok() {
            return (boundary, address.port());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("real TLS boundary did not listen");
}

pub fn qualify(test_name: &str, probe: impl Fn(&str) -> Result<Vec<u8>, String>) {
    if let Ok(endpoint) = std::env::var("LAYERX_TLS_QUAL_ENDPOINT") {
        let result = probe(&endpoint);
        if std::env::var("LAYERX_TLS_QUAL_EXPECT").as_deref() == Ok("trusted") {
            let bytes = result.unwrap_or_else(|error| panic!("trusted TLS response: {error}"));
            let value: serde_json::Value = serde_json::from_slice(&bytes)
                .unwrap_or_else(|error| panic!("real boundary response: {error}"));
            assert_eq!(
                value,
                serde_json::json!({"status":"live","service":"paxeer-boundary"})
            );
        } else {
            assert!(
                result.is_err(),
                "untrusted certificate or wrong hostname was accepted"
            );
        }
        println!("TLS client probe completed");
        return;
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|error| panic!("qualification clock: {error}"))
        .as_nanos();
    let directory = std::env::temp_dir().join(format!("tls-client-{}-{stamp}", std::process::id()));
    fs::create_dir(&directory).unwrap_or_else(|error| panic!("qualification directory: {error}"));
    let empty_roots = directory.join("empty-roots");
    fs::create_dir(&empty_roots).unwrap_or_else(|error| panic!("empty root directory: {error}"));
    certificates(&directory);
    fs::write(directory.join("empty-cert.pem"), [])
        .unwrap_or_else(|error| panic!("empty trust fixture: {error}"));
    fs::write(directory.join("empty-cert.der"), [])
        .unwrap_or_else(|error| panic!("empty DER fixture: {error}"));
    let (_boundary, port) = start(&directory, "server.der", "key.der");
    let (_ca_boundary, ca_port) = start(&directory, "cert.der", "ca-key.der");
    for (case, host, roots) in [
        ("trusted", "localhost", "cert.pem"),
        ("unrelated-root", "localhost", "other-cert.pem"),
        ("wrong-hostname", "127.0.0.1", "cert.pem"),
        ("missing-roots", "localhost", "missing-cert.pem"),
        ("empty-roots", "localhost", "empty-cert.pem"),
        ("ca-as-server", "localhost", "cert.pem"),
    ] {
        let output = Command::new(
            std::env::current_exe().unwrap_or_else(|error| panic!("test executable: {error}")),
        )
        .args(["--exact", test_name, "--nocapture", "--test-threads=1"])
        .env(
            "LAYERX_TLS_QUAL_ENDPOINT",
            format!(
                "https://{host}:{}",
                if case == "ca-as-server" {
                    ca_port
                } else {
                    port
                }
            ),
        )
        .env("LAYERX_TLS_QUAL_EXPECT", case)
        .env(
            "LAYERX_TLS_QUAL_CA_DER",
            directory.join(roots).with_extension("der"),
        )
        .env("SSL_CERT_FILE", directory.join(roots))
        .env("SSL_CERT_DIR", &empty_roots)
        .output()
        .unwrap_or_else(|error| panic!("start TLS client probe: {error}"));
        assert!(
            output.status.success(),
            "TLS case {case}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("TLS client probe completed"),
            "TLS probe must execute exactly the requested case"
        );
    }
}
