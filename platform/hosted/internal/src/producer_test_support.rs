use std::fs;
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::JoinHandle;
use std::time::Duration;

use layerx_platform_internal::tls::{Origin, Upstream};
use native_tls::{Certificate, Identity};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{RootCertStore, ServerConfig, ServerConnection, StreamOwned};
use zeroize::Zeroizing;

pub struct Tls {
    pub config: Arc<ServerConfig>,
    pub ca: Certificate,
    pub identity: Identity,
}

impl Tls {
    pub fn new(root: &Path) -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        fs::create_dir_all(root).unwrap_or_else(|error| panic!("{error}"));
        issue_certificates(root);
        let der = fs::read(root.join("cert.der")).unwrap_or_else(|error| panic!("{error}"));
        let ca = Certificate::from_der(&der).unwrap_or_else(|error| panic!("{error}"));
        let mut roots = RootCertStore::empty();
        roots
            .add(CertificateDer::from(der.clone()))
            .unwrap_or_else(|error| panic!("{error}"));
        let verifier = rustls::server::WebPkiClientVerifier::builder(Arc::new(roots))
            .allow_unauthenticated()
            .build()
            .unwrap_or_else(|error| panic!("{error}"));
        let config = ServerConfig::builder()
            .with_client_cert_verifier(verifier)
            .with_single_cert(
                vec![CertificateDer::from(der)],
                PrivateKeyDer::from(PrivatePkcs8KeyDer::from(
                    fs::read(root.join("key.der")).unwrap_or_else(|error| panic!("{error}")),
                )),
            )
            .unwrap_or_else(|error| panic!("{error}"));
        let identity = Identity::from_pkcs12(
            &fs::read(root.join("client.p12")).unwrap_or_else(|error| panic!("{error}")),
            "integration-only",
        )
        .unwrap_or_else(|error| panic!("{error}"));
        Self {
            config: Arc::new(config),
            ca,
            identity,
        }
    }

    pub fn upstream(&self, port: u16, token: &str) -> Upstream {
        Upstream::new(
            Origin::parse(&format!("https://localhost:{port}"))
                .unwrap_or_else(|error| panic!("{error}")),
            self.ca.clone(),
            Some(self.identity.clone()),
            Some(Zeroizing::new(token.to_owned())),
        )
    }
}

fn issue_certificates(root: &Path) {
    for args in [
        vec![
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-keyout",
            "key.pem",
            "-out",
            "cert.pem",
            "-days",
            "2",
            "-subj",
            "/CN=localhost",
            "-addext",
            "subjectAltName=DNS:localhost",
            "-addext",
            "basicConstraints=critical,CA:TRUE",
        ],
        vec![
            "x509", "-in", "cert.pem", "-outform", "DER", "-out", "cert.der",
        ],
        vec![
            "pkcs8", "-topk8", "-nocrypt", "-in", "key.pem", "-outform", "DER", "-out", "key.der",
        ],
        vec![
            "req",
            "-new",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-keyout",
            "client.key",
            "-out",
            "client.csr",
            "-subj",
            "/CN=producer",
        ],
    ] {
        run(root, &args);
    }
    fs::write(
        root.join("client.ext"),
        "basicConstraints=critical,CA:FALSE\nextendedKeyUsage=clientAuth\n",
    )
    .unwrap_or_else(|error| panic!("{error}"));
    run(
        root,
        &[
            "x509",
            "-req",
            "-in",
            "client.csr",
            "-CA",
            "cert.pem",
            "-CAkey",
            "key.pem",
            "-CAcreateserial",
            "-out",
            "client.pem",
            "-days",
            "2",
            "-extfile",
            "client.ext",
        ],
    );
    run(
        root,
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

fn run(root: &Path, args: &[&str]) {
    let output = Command::new("openssl")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(output.status.success(), "certificate generation failed");
}

pub struct Listener {
    pub port: u16,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Listener {
    pub fn start(
        tls: Arc<ServerConfig>,
        port: u16,
        handler: impl Fn(&mut StreamOwned<ServerConnection, TcpStream>) + Send + 'static,
    ) -> Self {
        let listener =
            TcpListener::bind(("127.0.0.1", port)).unwrap_or_else(|error| panic!("{error}"));
        let port = listener
            .local_addr()
            .unwrap_or_else(|error| panic!("{error}"))
            .port();
        listener
            .set_nonblocking(true)
            .unwrap_or_else(|error| panic!("{error}"));
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let worker = std::thread::spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((tcp, _)) => {
                        tcp.set_read_timeout(Some(Duration::from_secs(3)))
                            .unwrap_or_else(|error| panic!("{error}"));
                        tcp.set_write_timeout(Some(Duration::from_secs(3)))
                            .unwrap_or_else(|error| panic!("{error}"));
                        let conn = ServerConnection::new(Arc::clone(&tls))
                            .unwrap_or_else(|error| panic!("{error}"));
                        handler(&mut StreamOwned::new(conn, tcp));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("{error}"),
                }
            }
        });
        Self {
            port,
            stop,
            worker: Some(worker),
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            assert!(worker.join().is_ok());
        }
    }
}

pub struct Webhooks {
    redis: std::process::Child,
    child: Option<std::process::Child>,
    command: Command,
    pub port: u16,
}

impl Webhooks {
    pub fn start(root: &Path, sources: &[(&str, u16)]) -> Self {
        let port = free_port();
        let redis_port = free_port();
        fs::write(
            root.join("redis.acl"),
            "user default off\nuser producer on >integration-only ~* &* +@all\n",
        )
        .unwrap_or_else(|error| panic!("{error}"));
        fs::write(root.join("redis.conf"), format!("bind 127.0.0.1\nport 0\ntls-port {redis_port}\ntls-cert-file {}\ntls-key-file {}\ntls-ca-cert-file {}\ntls-auth-clients no\naclfile {}\nappendonly yes\nappendfsync always\ndir {}\n", root.join("cert.pem").display(), root.join("key.pem").display(), root.join("cert.pem").display(), root.join("redis.acl").display(), root.display())).unwrap_or_else(|error| panic!("{error}"));
        let redis = Command::new("redis-server")
            .arg(root.join("redis.conf"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap_or_else(|error| panic!("{error}"));
        let mut command = Command::new(
            std::env::var_os("LAYERX_TEST_WEBHOOKS_BIN")
                .unwrap_or_else(|| panic!("LAYERX_TEST_WEBHOOKS_BIN required")),
        );
        command
            .env_clear()
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let endpoint = format!("https://localhost:{}", sources[0].1);
        for (name, value) in [
            ("LISTEN", format!("127.0.0.1:{port}")),
            ("REDIS_URL", format!("rediss://localhost:{redis_port}")),
            ("KMS_URL", endpoint.clone()),
            ("IDENTITY_URL", endpoint.clone()),
            ("COMPONENT_URL", endpoint.clone()),
            ("AUTHORITY_URL", endpoint.clone()),
            ("INSTANCE_ID", "producer-integration".to_owned()),
            ("LXP_WIRE_VERSION", "3".to_owned()),
            ("NETWORK_ID", "77".to_owned()),
        ] {
            command.env(format!("LAYERX_WEBHOOKS_{name}"), value);
        }
        for (name, file) in [
            ("TLS_CERT_DER", "cert.der"),
            ("TLS_KEY_DER", "key.der"),
            ("INTERNAL_CA_DER", "cert.der"),
            ("PUBLIC_CA_DER", "cert.der"),
            ("CLIENT_IDENTITY_PKCS12", "client.p12"),
        ] {
            command.env(format!("LAYERX_WEBHOOKS_{name}"), root.join(file));
        }
        for (name, value) in [
            ("CLIENT_IDENTITY_PASSWORD", "integration-only".to_owned()),
            ("REDIS_USERNAME", "producer".to_owned()),
            ("REDIS_PASSWORD", "integration-only".to_owned()),
            ("CURSOR_KEY", "11".repeat(32)),
            ("KMS_TOKEN", "kms-integration".to_owned()),
            ("IDENTITY_TOKEN", "identity-integration".to_owned()),
            ("COMPONENT_TOKEN", "component-integration".to_owned()),
            ("AUTHORITY_TOKEN", "authority-integration".to_owned()),
            ("SOURCE_TRIGGER_TOKEN", "notification-token".to_owned()),
            ("OPERATOR_TOKEN", "operator-integration".to_owned()),
            (
                "SEQUENCER_PUBLIC_KEY",
                "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a".to_owned(),
            ),
            ("SEQUENCER_ID", "11".repeat(32)),
            ("SEQUENCER_FIRST_BATCH", "0".to_owned()),
            ("SEQUENCER_LAST_BATCH", u64::MAX.to_string()),
        ] {
            fs::write(root.join(name), value).unwrap_or_else(|error| panic!("{error}"));
            command.env(format!("LAYERX_WEBHOOKS_{name}_FILE"), root.join(name));
        }
        fs::write(root.join("consumer-token"), "consumer-token")
            .unwrap_or_else(|error| panic!("{error}"));
        for kind in ["JOURNEY", "APPROVAL", "PROGRAM", "PAYMENT"] {
            let source_port = sources
                .iter()
                .find(|(name, _)| *name == kind)
                .map_or(sources[0].1, |(_, port)| *port);
            command.env(
                format!("LAYERX_WEBHOOKS_{kind}_SOURCE_URL"),
                format!("https://localhost:{source_port}"),
            );
            command.env(
                format!("LAYERX_WEBHOOKS_{kind}_SOURCE_TOKEN_FILE"),
                root.join("consumer-token"),
            );
        }
        let mut result = Self {
            redis,
            child: None,
            command,
            port,
        };
        result.restart();
        result
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub fn restart(&mut self) {
        assert!(self.child.is_none());
        self.child = Some(
            self.command
                .spawn()
                .unwrap_or_else(|error| panic!("{error}")),
        );
        for _ in 0..100 {
            assert!(
                self.child
                    .as_mut()
                    .unwrap_or_else(|| panic!("child"))
                    .try_wait()
                    .unwrap_or_else(|error| panic!("child status: {error:?}"))
                    .is_none(),
                "webhook exited before listening"
            );
            if TcpStream::connect(("127.0.0.1", self.port)).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("webhook did not listen");
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap_or_else(|error| panic!("{error}"))
        .local_addr()
        .unwrap_or_else(|error| panic!("{error}"))
        .port()
}

impl Drop for Webhooks {
    fn drop(&mut self) {
        self.stop();
        let _ = self.redis.kill();
        let _ = self.redis.wait();
    }
}
