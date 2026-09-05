use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use layerx_platform_internal::base64;
use native_tls::{Certificate, Identity, TlsConnector};
use serde_json::{json, Value};

struct Fixture {
    directory: PathBuf,
    child: Child,
    port: u16,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.directory);
    }
}
fn command(directory: &Path, args: &[&str]) {
    let output = Command::new("openssl")
        .current_dir(directory)
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("openssl: {error}"));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn spawn(directory: &Path, port: u16) -> Child {
    Command::new(env!("CARGO_BIN_EXE_layerx-kms"))
        .env("LAYERX_KMS_LISTEN", format!("127.0.0.1:{port}"))
        .env("LAYERX_KMS_STATE_DIR", directory.join("state"))
        .env("LAYERX_KMS_TOKEN_FILE", directory.join("token"))
        .env("LAYERX_KMS_SEAL_SECRET_FILE", directory.join("seal"))
        .env("LAYERX_KMS_TLS_CERT_DER", directory.join("server.der"))
        .env("LAYERX_KMS_TLS_KEY_DER", directory.join("server-key.der"))
        .env("LAYERX_KMS_CLIENT_CA_DER", directory.join("ca.der"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|error| panic!("kms spawn: {error}"))
}
fn create_certificates(directory: &Path) {
    command(
        directory,
        &[
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-keyout",
            "ca.key",
            "-out",
            "ca.pem",
            "-days",
            "1",
            "-subj",
            "/CN=LayerX Test CA",
            "-addext",
            "basicConstraints=critical,CA:TRUE",
        ],
    );
    fs::write(directory.join("server.ext"), "basicConstraints=critical,CA:FALSE\nsubjectAltName=DNS:localhost\nextendedKeyUsage=serverAuth\n").unwrap_or_else(|error| panic!("ext: {error}"));
    fs::write(
        directory.join("client.ext"),
        "basicConstraints=critical,CA:FALSE\nextendedKeyUsage=clientAuth\n",
    )
    .unwrap_or_else(|error| panic!("ext: {error}"));
    for name in ["server", "client"] {
        command(
            directory,
            &[
                "req",
                "-new",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-keyout",
                &format!("{name}.key"),
                "-out",
                &format!("{name}.csr"),
                "-subj",
                &format!("/CN={name}"),
            ],
        );
        command(
            directory,
            &[
                "x509",
                "-req",
                "-in",
                &format!("{name}.csr"),
                "-CA",
                "ca.pem",
                "-CAkey",
                "ca.key",
                "-CAcreateserial",
                "-out",
                &format!("{name}.pem"),
                "-days",
                "1",
                "-extfile",
                &format!("{name}.ext"),
            ],
        );
    }
}
fn convert_certificates(directory: &Path) {
    for name in ["server", "ca"] {
        command(
            directory,
            &[
                "x509",
                "-in",
                &format!("{name}.pem"),
                "-outform",
                "DER",
                "-out",
                &format!("{name}.der"),
            ],
        );
    }
    command(
        directory,
        &[
            "pkcs8",
            "-topk8",
            "-nocrypt",
            "-in",
            "server.key",
            "-outform",
            "DER",
            "-out",
            "server-key.der",
        ],
    );
    command(
        directory,
        &[
            "pkcs12",
            "-export",
            "-inkey",
            "client.key",
            "-in",
            "client.pem",
            "-out",
            "client.p12",
            "-passout",
            "pass:integration-only",
        ],
    );
}
impl Fixture {
    fn start() -> Self {
        let directory = std::env::temp_dir().join(format!("layerx-kms-tls-{}", std::process::id()));
        fs::create_dir(&directory).unwrap_or_else(|error| panic!("directory: {error}"));
        create_certificates(&directory);
        convert_certificates(&directory);
        fs::write(directory.join("token"), "kms-integration-token")
            .unwrap_or_else(|error| panic!("token: {error}"));
        fs::write(
            directory.join("seal"),
            "kms-integration-seal-secret-32-bytes",
        )
        .unwrap_or_else(|error| panic!("seal: {error}"));
        let listener =
            TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("port: {error}"));
        let port = listener
            .local_addr()
            .unwrap_or_else(|error| panic!("address: {error}"))
            .port();
        drop(listener);
        let child = spawn(&directory, port);
        let fixture = Self {
            directory,
            child,
            port,
        };
        fixture.wait();
        fixture
    }
    fn wait(&self) {
        for _ in 0..100 {
            if TcpStream::connect(("127.0.0.1", self.port)).is_ok() {
                return;
            }
            thread::sleep(Duration::from_millis(25));
        }
        panic!("KMS never listened");
    }
    fn request(&self, path: &str, token: &str, certificate: bool, body: &Value) -> (u16, Value) {
        let ca =
            fs::read(self.directory.join("ca.der")).unwrap_or_else(|error| panic!("CA: {error}"));
        let mut builder = TlsConnector::builder();
        builder.disable_built_in_roots(true).add_root_certificate(
            Certificate::from_der(&ca).unwrap_or_else(|error| panic!("CA: {error}")),
        );
        if certificate {
            builder.identity(
                Identity::from_pkcs12(
                    &fs::read(self.directory.join("client.p12"))
                        .unwrap_or_else(|error| panic!("identity: {error}")),
                    "integration-only",
                )
                .unwrap_or_else(|error| panic!("identity: {error}")),
            );
        }
        let connector = builder
            .build()
            .unwrap_or_else(|error| panic!("TLS: {error}"));
        let tcp = TcpStream::connect(("127.0.0.1", self.port))
            .unwrap_or_else(|error| panic!("connect: {error}"));
        tcp.set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap_or_else(|error| panic!("timeout: {error}"));
        let mut tls = connector
            .connect("localhost", tcp)
            .unwrap_or_else(|error| panic!("TLS connect: {error}"));
        let method = if matches!(path, "/readyz" | "/livez") {
            "GET"
        } else {
            "POST"
        };
        let body = body.to_string();
        write!(tls, "{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\nIdempotency-Key: registration-one\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap_or_else(|error| panic!("request: {error}"));
        tls.flush().unwrap_or_else(|error| panic!("flush: {error}"));
        let mut bytes = Vec::new();
        tls.read_to_end(&mut bytes)
            .unwrap_or_else(|error| panic!("response: {error}"));
        let response =
            String::from_utf8(bytes).unwrap_or_else(|error| panic!("response utf8: {error}"));
        let (headers, body) = response
            .split_once("\r\n\r\n")
            .unwrap_or_else(|| panic!("framed response"));
        let status = headers
            .split_whitespace()
            .nth(1)
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(|| panic!("status"));
        (
            status,
            serde_json::from_str(body).unwrap_or_else(|error| panic!("JSON: {error}")),
        )
    }
}

#[test]
fn real_tls_kms_refuses_missing_credentials_and_preserves_signing_identity_after_restart() {
    let mut fixture = Fixture::start();
    assert_eq!(
        fixture.request("/readyz", "", false, &json!({})),
        (200, json!({"ready":true,"ed25519_non_exportable":true}))
    );
    assert_eq!(
        fixture.request("/livez", "", false, &json!({})),
        (200, json!({"alive":true}))
    );
    let create = json!({"algorithm":"ed25519","purpose":"layerx-webhook-v1"});
    assert_eq!(
        fixture
            .request("/v1/signing-keys", "kms-integration-token", false, &create)
            .0,
        401
    );
    assert_eq!(
        fixture
            .request("/v1/signing-keys", "wrong-token", true, &create)
            .0,
        401
    );
    let (status, key) = fixture.request("/v1/signing-keys", "kms-integration-token", true, &create);
    assert_eq!(status, 201);
    assert_eq!(
        fixture.request("/v1/signing-keys", "kms-integration-token", true, &create),
        (200, key.clone())
    );
    assert!(key.get("seed").is_none());
    fixture
        .child
        .kill()
        .unwrap_or_else(|error| panic!("stop: {error}"));
    fixture
        .child
        .wait()
        .unwrap_or_else(|error| panic!("wait: {error}"));
    fixture.child = spawn(&fixture.directory, fixture.port);
    fixture.wait();
    assert_eq!(
        fixture.request("/v1/signing-keys", "kms-integration-token", true, &create),
        (200, key.clone())
    );
    let (status, signature) = fixture.request("/v1/signatures", "kms-integration-token", true, &json!({"algorithm":"ed25519","key_handle":key["handle"],"message":base64::encode(b"real webhook message")}));
    assert_eq!(status, 200);
    let decode = |value: &Value| {
        base64::decode(value.as_str().unwrap_or_else(|| panic!("base64 string")))
            .unwrap_or_else(|| panic!("base64"))
    };
    let public: [u8; 32] = decode(&key["public_key"])
        .try_into()
        .unwrap_or_else(|_| panic!("public length"));
    let signature: [u8; 64] = decode(&signature["signature"])
        .try_into()
        .unwrap_or_else(|_| panic!("signature length"));
    let public =
        VerifyingKey::from_bytes(&public).unwrap_or_else(|error| panic!("public: {error}"));
    assert!(public
        .verify(b"real webhook message", &Signature::from_bytes(&signature))
        .is_ok());
    assert!(public
        .verify(
            b"tampered webhook message",
            &Signature::from_bytes(&signature)
        )
        .is_err());
}
