use std::fmt::Write as _;
use std::future::Future;
use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::pin::pin;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, JoinHandle};
use std::time::Duration;

mod support;

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_crypto::disclosure::bind;
use layerx_crypto::remote::RemoteSigner;
use layerx_crypto::signer::{sign_disclosed, SignError};
use openssl::ssl::{SslAcceptor, SslFiletype, SslMethod, SslVerifyMode};

static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

struct NoopWake;

impl Wake for NoopWake {
    fn wake(self: Arc<Self>) {}
}

fn ready<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::from(Arc::new(NoopWake));
    let mut context = Context::from_waker(&waker);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("remote signer future unexpectedly blocked"),
    }
}

struct Certificates {
    directory: std::path::PathBuf,
}

impl Certificates {
    fn create() -> Self {
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "layerx-remote-signer-{}-{sequence}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        assert!(std::fs::create_dir(&directory).is_ok());
        let ca_key = directory.join("ca.key");
        let ca_cert = directory.join("ca.pem");
        run(Command::new("openssl")
            .args(["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-sha256"])
            .args(["-days", "1", "-subj", "/CN=LayerX Test CA", "-keyout"])
            .arg(&ca_key)
            .arg("-out")
            .arg(&ca_cert));
        issue_certificate(&directory, "server", "serverAuth", Some("DNS:localhost"));
        issue_certificate(&directory, "client", "clientAuth", None);
        Self { directory }
    }

    fn path(&self, name: &str) -> std::path::PathBuf {
        self.directory.join(name)
    }
}

impl Drop for Certificates {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

fn run(command: &mut Command) {
    let status = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    assert!(status.is_ok_and(|value| value.success()));
}

fn issue_certificate(
    directory: &std::path::Path,
    name: &str,
    usage: &str,
    subject_alt_name: Option<&str>,
) {
    let key = directory.join(format!("{name}.key"));
    let request = directory.join(format!("{name}.csr"));
    let certificate = directory.join(format!("{name}.pem"));
    let extensions = directory.join(format!("{name}.ext"));
    let mut extension_text = format!("extendedKeyUsage={usage}\n");
    if let Some(subject_alt_name) = subject_alt_name {
        assert!(writeln!(extension_text, "subjectAltName={subject_alt_name}").is_ok());
    }
    assert!(std::fs::write(&extensions, extension_text).is_ok());
    run(Command::new("openssl")
        .args(["req", "-newkey", "rsa:2048", "-nodes", "-sha256"])
        .args(["-subj", &format!("/CN={name}"), "-keyout"])
        .arg(&key)
        .arg("-out")
        .arg(&request));
    run(Command::new("openssl")
        .args(["x509", "-req", "-sha256", "-days", "1", "-in"])
        .arg(&request)
        .arg("-CA")
        .arg(directory.join("ca.pem"))
        .arg("-CAkey")
        .arg(directory.join("ca.key"))
        .arg("-CAcreateserial")
        .arg("-extfile")
        .arg(&extensions)
        .arg("-out")
        .arg(&certificate));
}

#[derive(Clone, Copy)]
enum Behavior {
    Sign,
    Refuse,
    Slow,
    Hostile,
    Malformed,
    Drop,
}

fn acceptor(certificates: &Certificates) -> SslAcceptor {
    let Ok(mut acceptor) = SslAcceptor::mozilla_intermediate(SslMethod::tls_server()) else {
        panic!("TLS acceptor construction failed");
    };
    assert!(acceptor
        .set_private_key_file(certificates.path("server.key"), SslFiletype::PEM)
        .is_ok());
    assert!(acceptor
        .set_certificate_chain_file(certificates.path("server.pem"))
        .is_ok());
    assert!(acceptor.set_ca_file(certificates.path("ca.pem")).is_ok());
    acceptor.set_verify(SslVerifyMode::PEER | SslVerifyMode::FAIL_IF_NO_PEER_CERT);
    assert!(acceptor.check_private_key().is_ok());
    acceptor.build()
}

fn spawn_server(
    certificates: &Certificates,
    behavior: Behavior,
) -> (std::net::SocketAddr, Arc<AtomicUsize>, JoinHandle<()>) {
    let Ok(listener) = TcpListener::bind("127.0.0.1:0") else {
        panic!("test signer listener could not bind");
    };
    let Ok(address) = listener.local_addr() else {
        panic!("test signer address unavailable");
    };
    let acceptor = acceptor(certificates);
    let attempts = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&attempts);
    let handle = thread::spawn(move || {
        let Ok((stream, _)) = listener.accept() else {
            return;
        };
        let Ok(mut tls) = acceptor.accept(stream) else {
            return;
        };
        let mut length = [0_u8; 4];
        if tls.read_exact(&mut length).is_err() {
            return;
        }
        let Ok(length) = usize::try_from(u32::from_be_bytes(length)) else {
            return;
        };
        let mut request = vec![0_u8; length];
        if tls.read_exact(&mut request).is_err() {
            return;
        }
        observed.fetch_add(1, Ordering::SeqCst);
        let Some(digest) = parse_request(&request) else {
            return;
        };
        if matches!(behavior, Behavior::Drop) {
            return;
        }
        if matches!(behavior, Behavior::Slow) {
            thread::sleep(Duration::from_millis(250));
        }
        let response = match behavior {
            Behavior::Refuse => vec![0, 0, 0, 1, 1],
            Behavior::Malformed => vec![0, 0, 0, 2, 0, 0],
            Behavior::Hostile => {
                let mut response = vec![0, 0, 0, 65, 0];
                response.extend_from_slice(&[0x99; 64]);
                response
            }
            Behavior::Sign | Behavior::Slow => {
                let signature = SigningKey::from_bytes(&[0x67; 32]).sign(&digest);
                let mut response = vec![0, 0, 0, 65, 0];
                response.extend_from_slice(&signature.to_bytes());
                response
            }
            Behavior::Drop => return,
        };
        let _ = tls.write_all(&response);
        let _ = tls.flush();
    });
    (address, attempts, handle)
}

fn parse_request(request: &[u8]) -> Option<[u8; 32]> {
    if request.get(..4)? != b"LXRS" || request.get(4..6)? != 1_u16.to_be_bytes() {
        return None;
    }
    let digest: [u8; 32] = request.get(12..44)?.try_into().ok()?;
    let canonical_length =
        usize::try_from(u32::from_be_bytes(request.get(44..48)?.try_into().ok()?)).ok()?;
    let canonical_end = 48_usize.checked_add(canonical_length)?;
    if canonical_length == 0 || canonical_end > request.len() {
        return None;
    }
    let disclosure_length = usize::try_from(u32::from_be_bytes(
        request
            .get(canonical_end..canonical_end + 4)?
            .try_into()
            .ok()?,
    ))
    .ok()?;
    let disclosure_start = canonical_end.checked_add(4)?;
    let disclosure_end = disclosure_start.checked_add(disclosure_length)?;
    if disclosure_length == 0 || disclosure_end != request.len() {
        return None;
    }
    Some(digest)
}

fn remote(
    certificates: &Certificates,
    address: std::net::SocketAddr,
    timeout: Duration,
) -> RemoteSigner {
    let public_key = SigningKey::from_bytes(&[0x67; 32])
        .verifying_key()
        .to_bytes();
    let Ok(signer) = RemoteSigner::new(
        address,
        "localhost",
        public_key,
        certificates.path("ca.pem"),
        certificates.path("client.pem"),
        certificates.path("client.key"),
        timeout,
    ) else {
        panic!("valid remote signer TLS configuration rejected");
    };
    signer
}

fn remote_with_name(
    certificates: &Certificates,
    address: std::net::SocketAddr,
    server_name: &str,
) -> RemoteSigner {
    let public_key = SigningKey::from_bytes(&[0x67; 32])
        .verifying_key()
        .to_bytes();
    let Ok(signer) = RemoteSigner::new(
        address,
        server_name,
        public_key,
        certificates.path("ca.pem"),
        certificates.path("client.pem"),
        certificates.path("client.key"),
        Duration::from_secs(2),
    ) else {
        panic!("valid remote signer TLS material rejected");
    };
    signer
}

fn sign(signer: &RemoteSigner) -> Result<layerx_crypto::signer::AgentSignature, SignError> {
    let canonical = support::canonical_send(25);
    let registry = support::registry();
    let Ok(disclosure) = bind(&canonical, &registry) else {
        panic!("canonical disclosure rejected");
    };
    ready(sign_disclosed(signer, &canonical, &disclosure, &registry))
}

#[test]
fn mutually_authenticated_remote_signer_returns_one_verified_signature() {
    let certificates = Certificates::create();
    let (address, attempts, handle) = spawn_server(&certificates, Behavior::Sign);
    let signer = remote(&certificates, address, Duration::from_secs(2));
    assert!(sign(&signer).is_ok());
    assert!(handle.join().is_ok());
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}

#[test]
fn server_identity_mismatch_is_an_authentication_refusal() {
    let certificates = Certificates::create();
    let (address, attempts, handle) = spawn_server(&certificates, Behavior::Sign);
    let signer = remote_with_name(&certificates, address, "different.example");
    assert_eq!(sign(&signer), Err(SignError::RemoteAuthentication));
    assert!(handle.join().is_ok());
    assert_eq!(attempts.load(Ordering::SeqCst), 0);
}

#[test]
fn refusal_timeout_and_malformed_response_remain_distinct() {
    for (behavior, expected, timeout) in [
        (
            Behavior::Refuse,
            SignError::RemoteRefused,
            Duration::from_secs(2),
        ),
        (
            Behavior::Slow,
            SignError::RemoteTimeout,
            Duration::from_millis(50),
        ),
        (
            Behavior::Malformed,
            SignError::RemoteMalformedResponse,
            Duration::from_secs(2),
        ),
    ] {
        let certificates = Certificates::create();
        let (address, attempts, handle) = spawn_server(&certificates, behavior);
        let signer = remote(&certificates, address, timeout);
        assert_eq!(sign(&signer), Err(expected));
        assert!(handle.join().is_ok());
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn hostile_signature_is_verified_and_rejected() {
    let certificates = Certificates::create();
    let (address, attempts, handle) = spawn_server(&certificates, Behavior::Hostile);
    let signer = remote(&certificates, address, Duration::from_secs(2));
    assert_eq!(sign(&signer), Err(SignError::ReturnedSignatureInvalid));
    assert!(handle.join().is_ok());
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}

#[test]
fn signer_removed_midflight_has_no_retry_cache_or_fallback() {
    let certificates = Certificates::create();
    let (address, attempts, handle) = spawn_server(&certificates, Behavior::Drop);
    let signer = remote(&certificates, address, Duration::from_secs(2));
    assert_eq!(sign(&signer), Err(SignError::RemoteUnavailable));
    assert!(handle.join().is_ok());
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}

#[test]
fn unreachable_signer_is_a_typed_refusal_without_artifact() {
    let certificates = Certificates::create();
    let Ok(listener) = TcpListener::bind("127.0.0.1:0") else {
        panic!("temporary endpoint could not bind");
    };
    let Ok(address) = listener.local_addr() else {
        panic!("temporary endpoint address unavailable");
    };
    drop(listener);
    let signer = remote(&certificates, address, Duration::from_millis(100));
    assert_eq!(sign(&signer), Err(SignError::RemoteUnavailable));
}

#[test]
fn handshake_exceeding_the_deadline_is_a_timeout_not_an_authentication_refusal() {
    let certificates = Certificates::create();
    let Ok(listener) = TcpListener::bind("127.0.0.1:0") else {
        panic!("silent endpoint could not bind");
    };
    let Ok(address) = listener.local_addr() else {
        panic!("silent endpoint address unavailable");
    };
    let handle = thread::spawn(move || {
        let Ok((stream, _)) = listener.accept() else {
            return;
        };
        thread::sleep(Duration::from_millis(400));
        drop(stream);
    });
    let signer = remote(&certificates, address, Duration::from_millis(100));
    assert_eq!(sign(&signer), Err(SignError::RemoteTimeout));
    assert!(handle.join().is_ok());
}
