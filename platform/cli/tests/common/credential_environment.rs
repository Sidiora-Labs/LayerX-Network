use std::io::{BufRead as _, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub struct CredentialEnvironment {
    root: PathBuf,
    address: String,
    bus: Option<Child>,
    keyring: Option<Child>,
}

impl CredentialEnvironment {
    pub fn new() -> Self {
        let root = super::scratch("credentials");
        let mut directory = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt as _;
            directory.mode(0o700);
        }
        directory
            .create(&root)
            .unwrap_or_else(|error| panic!("private credential directory should be new: {error}"));
        let mut environment = Self {
            root,
            address: String::new(),
            bus: None,
            keyring: None,
        };
        for directory in ["config", "data", "cache", "runtime", "control"] {
            std::fs::create_dir(environment.root.join(directory)).unwrap_or_else(|error| {
                panic!("isolated XDG directory should be created: {error}")
            });
        }
        let config = environment.root.join("bus.conf");
        std::fs::write(
            &config,
            "<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth><policy context=\"default\"><allow send_destination=\"*\"/><allow receive_sender=\"*\"/><allow own=\"*\"/></policy></busconfig>",
        )
        .unwrap_or_else(|error| panic!("private bus configuration should be writable: {error}"));
        let bus = environment
            .command("dbus-daemon")
            .arg("--config-file")
            .arg(config)
            .arg(format!(
                "--address=unix:path={}",
                environment.root.join("bus").display()
            ))
            .args(["--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap_or_else(|error| {
                panic!("dbus-daemon must be installed for real credential tests: {error}")
            });
        environment.bus = Some(bus);
        let stdout = environment
            .bus
            .as_mut()
            .and_then(|child| child.stdout.take())
            .unwrap_or_else(|| panic!("private bus should expose its address"));
        BufReader::new(stdout)
            .read_line(&mut environment.address)
            .unwrap_or_else(|error| panic!("private bus address should be readable: {error}"));
        environment.address = environment.address.trim().to_owned();
        assert!(
            environment.address.starts_with("unix:"),
            "private bus must start"
        );
        let keyring = environment
            .command("gnome-keyring-daemon")
            .args([
                "--foreground",
                "--unlock",
                "--components=secrets",
                "--control-directory",
            ])
            .arg(environment.root.join("control"))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|error| {
                panic!("gnome-keyring-daemon must be installed for real credential tests: {error}")
            });
        environment.keyring = Some(keyring);
        let mut input = environment
            .keyring
            .as_mut()
            .and_then(|child| child.stdin.take())
            .unwrap_or_else(|| panic!("private keyring should accept unlock input"));
        input
            .write_all(b"\n")
            .unwrap_or_else(|error| panic!("private keyring unlock input failed: {error}"));
        drop(input);
        environment.wait_ready();
        environment
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn command(&self, binary: &str) -> Command {
        let mut command = Command::new(binary);
        command.env_clear();
        for name in ["PATH", "LD_LIBRARY_PATH"] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        command
            .env("HOME", &self.root)
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("XDG_DATA_HOME", self.root.join("data"))
            .env("XDG_CACHE_HOME", self.root.join("cache"))
            .env("XDG_RUNTIME_DIR", self.root.join("runtime"))
            .env("DBUS_SESSION_BUS_ADDRESS", &self.address);
        command
    }

    fn wait_ready(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            let output = self
                .command("dbus-send")
                .args([
                    "--session",
                    "--print-reply",
                    "--reply-timeout=1000",
                    "--dest=org.freedesktop.secrets",
                    "/org/freedesktop/secrets",
                    "org.freedesktop.Secret.Service.ReadAlias",
                    "string:default",
                ])
                .output()
                .unwrap_or_else(|error| {
                    panic!("dbus-send must be installed for real credential tests: {error}")
                });
            if output.status.success()
                && String::from_utf8_lossy(&output.stdout)
                    .contains("/org/freedesktop/secrets/collection/")
            {
                return;
            }
            assert!(
                self.keyring
                    .as_mut()
                    .unwrap_or_else(|| panic!("keyring should be owned"))
                    .try_wait()
                    .unwrap_or_else(|error| panic!("keyring status should be available: {error}"))
                    .is_none(),
                "private keyring exited before readiness"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("private Secret Service did not create its default collection");
    }
}

impl Drop for CredentialEnvironment {
    fn drop(&mut self) {
        for child in [&mut self.keyring, &mut self.bus].into_iter().flatten() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
