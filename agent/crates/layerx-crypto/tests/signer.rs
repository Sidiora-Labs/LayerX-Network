use std::future::Future;
use std::pin::pin;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;

mod support;

use layerx_crypto::disclosure::bind;
use layerx_crypto::ed25519;
use layerx_crypto::signer::{sign_disclosed, KeystoreSigner, LocalSigner, SignError, Signer};
use layerx_crypto::SignatureMessage;
use layerx_wire::hash::Domain;
use layerx_wire::limits::PROTOCOL_VERSION;

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
        Poll::Pending => panic!("signer future unexpectedly blocked"),
    }
}

#[test]
fn local_signer_is_object_safe_and_never_renders_key_material() {
    let seed = [0x53; 32];
    let signer: Box<dyn Signer> = Box::new(LocalSigner::new(seed));
    let rendered = format!("{signer:?}");
    assert_eq!(rendered, "LocalSigner([redacted key])");
    assert!(!rendered.contains("53535353"));
    let canonical = support::canonical_send(25);
    let registry = support::registry();
    let Ok(disclosure) = bind(&canonical, &registry) else {
        panic!("canonical send disclosure rejected");
    };
    let signature = ready(sign_disclosed(
        signer.as_ref(),
        &canonical,
        &disclosure,
        &registry,
    ));
    let Ok(signature) = signature else {
        panic!("local signer refused valid request");
    };
    let Ok(message) =
        SignatureMessage::new(Domain::SignaturePreimage, PROTOCOL_VERSION, 17, &canonical)
    else {
        panic!("valid verification message rejected");
    };
    assert_eq!(
        ed25519::verify(&signer.public_key(), signature.as_bytes(), message),
        Ok(())
    );
    let panic = std::panic::catch_unwind(|| panic!("{rendered}"));
    let Err(payload) = panic else {
        panic!("redaction panic did not occur");
    };
    let Some(text) = payload.downcast_ref::<String>() else {
        panic!("panic payload was not owned text");
    };
    assert!(!text.contains("53535353"));
    assert!(!SignError::KeyRejected.to_string().contains("53535353"));
}

#[test]
fn mismatched_disclosure_is_refused_before_a_signer_runs() {
    let canonical = support::canonical_send(25);
    let registry = support::registry();
    let Ok(mut disclosure) = bind(&canonical, &registry) else {
        panic!("canonical send disclosure rejected");
    };
    disclosure.amounts[0].value = 1;
    let signer = LocalSigner::new([0x53; 32]);
    assert_eq!(
        ready(sign_disclosed(&signer, &canonical, &disclosure, &registry,)),
        Err(SignError::DisclosureMismatch("amounts"))
    );
}

#[cfg(unix)]
struct AgentFixture {
    child: Child,
    directory: std::path::PathBuf,
}

#[cfg(unix)]
impl Drop for AgentFixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[cfg(unix)]
fn decode_base64(value: &str) -> Option<Vec<u8>> {
    fn digit(value: u8) -> Option<u8> {
        match value {
            b'A'..=b'Z' => Some(value - b'A'),
            b'a'..=b'z' => Some(value - b'a' + 26),
            b'0'..=b'9' => Some(value - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }

    let mut output = Vec::new();
    for chunk in value.as_bytes().chunks(4) {
        if chunk.len() != 4 {
            return None;
        }
        let a = digit(chunk[0])?;
        let b = digit(chunk[1])?;
        output.push((a << 2) | (b >> 4));
        if chunk[2] != b'=' {
            let c = digit(chunk[2])?;
            output.push((b << 4) | (c >> 2));
            if chunk[3] != b'=' {
                let d = digit(chunk[3])?;
                output.push((c << 6) | d);
            }
        }
    }
    Some(output)
}

#[cfg(unix)]
fn public_key(path: &std::path::Path) -> [u8; 32] {
    let Ok(text) = std::fs::read_to_string(path) else {
        panic!("generated public key could not be read");
    };
    let Some(encoded) = text.split_whitespace().nth(1) else {
        panic!("generated public key lacks its blob");
    };
    let Some(blob) = decode_base64(encoded) else {
        panic!("generated public key blob is invalid base64");
    };
    let Some(raw) = blob.get(blob.len().saturating_sub(32)..) else {
        panic!("generated public key blob is too short");
    };
    let Ok(raw) = raw.try_into() else {
        panic!("generated public key has the wrong width");
    };
    raw
}

#[cfg(unix)]
#[test]
fn operating_system_keystore_signer_uses_a_real_ssh_agent() {
    let directory =
        std::env::temp_dir().join(format!("layerx-keystore-signer-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    assert!(std::fs::create_dir(&directory).is_ok());
    let key_path = directory.join("identity");
    let socket_path = directory.join("agent.sock");
    let status = Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-f"])
        .arg(&key_path)
        .status();
    assert!(status.is_ok_and(|value| value.success()));
    let child = Command::new("ssh-agent")
        .args(["-D", "-a"])
        .arg(&socket_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let Ok(child) = child else {
        panic!("real ssh-agent could not be started");
    };
    let fixture = AgentFixture { child, directory };
    for _ in 0..100 {
        if socket_path.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(socket_path.exists());
    let status = Command::new("ssh-add")
        .arg(&key_path)
        .env("SSH_AUTH_SOCK", &socket_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    assert!(status.is_ok_and(|value| value.success()));
    let public_key = public_key(&key_path.with_extension("pub"));
    let Ok(signer) = KeystoreSigner::new(&socket_path, public_key) else {
        panic!("real external key was rejected");
    };
    assert_eq!(format!("{signer:?}"), "KeystoreSigner([external key])");
    let canonical = support::canonical_send(25);
    let registry = support::registry();
    let Ok(disclosure) = bind(&canonical, &registry) else {
        panic!("canonical send disclosure rejected");
    };
    let signature = ready(sign_disclosed(&signer, &canonical, &disclosure, &registry));
    let Ok(signature) = signature else {
        panic!("real ssh-agent refused valid request");
    };
    let Ok(message) =
        SignatureMessage::new(Domain::SignaturePreimage, PROTOCOL_VERSION, 17, &canonical)
    else {
        panic!("valid verification message rejected");
    };
    assert_eq!(
        ed25519::verify(&public_key, signature.as_bytes(), message),
        Ok(())
    );
    drop(fixture);
}
