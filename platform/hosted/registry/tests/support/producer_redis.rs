use layerx_platform_gateway::store::{RedisEndpoint, RedisStore};
use native_tls::Certificate;
use std::fs;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zeroize::Zeroizing;

pub struct RedisProcess {
    child: Child,
    directory: PathBuf,
    endpoint: RedisEndpoint,
    certificate: Certificate,
}

impl RedisProcess {
    pub fn start() -> Self {
        let unique = format!(
            "layerx-tap-redis-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let directory = std::env::temp_dir().join(unique);
        fs::create_dir(&directory)
            .unwrap_or_else(|error| panic!("test Redis directory must be created: {error}"));
        let certificate_pem = directory.join("server.pem");
        let certificate_der = directory.join("server.der");
        let private_key = directory.join("server.key");
        command(
            "openssl",
            &[
                "req",
                "-x509",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-keyout",
                path(&private_key),
                "-out",
                path(&certificate_pem),
                "-days",
                "1",
                "-subj",
                "/CN=localhost",
                "-addext",
                "subjectAltName=DNS:localhost",
            ],
        );
        command(
            "openssl",
            &[
                "x509",
                "-in",
                path(&certificate_pem),
                "-outform",
                "DER",
                "-out",
                path(&certificate_der),
            ],
        );
        let listener = TcpListener::bind("127.0.0.1:0")
            .unwrap_or_else(|error| panic!("test port must be allocated: {error}"));
        let port = listener
            .local_addr()
            .unwrap_or_else(|error| panic!("test port must resolve: {error}"))
            .port();
        drop(listener);
        let acl = directory.join("users.acl");
        fs::write(
            &acl,
            "user default off\nuser tap on >tap-secret ~* &* +@all\n",
        )
        .unwrap_or_else(|error| panic!("test Redis ACL must be written: {error}"));
        let config = directory.join("redis.conf");
        fs::write(
            &config,
            format!(
                "bind 127.0.0.1\nport 0\ntls-port {port}\ntls-cert-file {}\ntls-key-file {}\ntls-ca-cert-file {}\ntls-auth-clients no\naclfile {}\nappendonly yes\nappendfsync always\ndir {}\nprotected-mode yes\n",
                path(&certificate_pem),
                path(&private_key),
                path(&certificate_pem),
                path(&acl),
                path(&directory),
            ),
        )
        .unwrap_or_else(|error| panic!("test Redis config must be written: {error}"));
        let endpoint = RedisEndpoint::parse(&format!("rediss://localhost:{port}"))
            .unwrap_or_else(|error| panic!("test Redis endpoint must parse: {error}"));
        let certificate = Certificate::from_der(
            &fs::read(&certificate_der)
                .unwrap_or_else(|error| panic!("test certificate must be read: {error}")),
        )
        .unwrap_or_else(|error| panic!("test certificate must parse: {error}"));
        let child = Command::new("redis-server")
            .arg(&config)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|error| panic!("real Redis server must start: {error}"));
        let process = Self {
            child,
            directory,
            endpoint,
            certificate,
        };
        for _ in 0..100 {
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                return process;
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!("Redis did not listen")
    }

    pub fn store(&self) -> RedisStore {
        RedisStore::new(
            self.endpoint.clone(),
            self.certificate.clone(),
            Zeroizing::new("tap".to_owned()),
            Zeroizing::new("tap-secret".to_owned()),
        )
    }
}

impl Drop for RedisProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn command(program: &str, arguments: &[&str]) {
    let output = Command::new(program)
        .args(arguments)
        .output()
        .unwrap_or_else(|error| panic!("{program} must run: {error}"));
    assert!(
        output.status.success(),
        "{program} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn path(value: &Path) -> &str {
    value
        .to_str()
        .unwrap_or_else(|| panic!("test path must be UTF-8"))
}
