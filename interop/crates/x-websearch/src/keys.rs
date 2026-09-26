use ed25519_dalek::SigningKey as ReceiverKey;
use k256::ecdsa::SigningKey as SecpKey;
use std::ffi::OsString;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

pub const ATTESTOR_KEY_FILE: &str = "X_WEBSEARCH_ATTESTOR_KEY_FILE";
pub const SUBMITTER_KEY_FILE: &str = "X_WEBSEARCH_SUBMITTER_KEY_FILE";
pub const RECEIVER_KEY_FILE: &str = "X_WEBSEARCH_RECEIVER_KEY_FILE";
pub const MAX_KEY_FILE_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyRole {
    Attestor,
    Submitter,
    Receiver,
}

impl KeyRole {
    #[must_use]
    pub const fn variable(self) -> &'static str {
        match self {
            Self::Attestor => ATTESTOR_KEY_FILE,
            Self::Submitter => SUBMITTER_KEY_FILE,
            Self::Receiver => RECEIVER_KEY_FILE,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyRefusal {
    Unset,
    Unreadable,
    Permissions,
    Oversized,
    Malformed,
    InvalidKey,
    SameKeyAs(KeyRole),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyError {
    pub role: KeyRole,
    pub refusal: KeyRefusal,
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let variable = self.role.variable();
        match self.refusal {
            KeyRefusal::Unset => write!(f, "key refused: {variable} is not set"),
            KeyRefusal::Unreadable => {
                write!(f, "key refused: the file {variable} names is unreadable")
            }
            KeyRefusal::Permissions => write!(
                f,
                "key refused: the file {variable} names is readable by group or others"
            ),
            KeyRefusal::Oversized => {
                write!(f, "key refused: the file {variable} names is too large")
            }
            KeyRefusal::Malformed => write!(
                f,
                "key refused: the file {variable} names does not hold 32 hexadecimal bytes"
            ),
            KeyRefusal::InvalidKey => {
                write!(
                    f,
                    "key refused: the file {variable} names holds an invalid key"
                )
            }
            KeyRefusal::SameKeyAs(other) => write!(
                f,
                "key refused: the file {variable} names holds the same key as {}",
                other.variable()
            ),
        }
    }
}

impl std::error::Error for KeyError {}

/// The key files the environment names. The receiver key is required; the
/// attestor and submitter keys are held only by attesting sidecars.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyFiles {
    pub attestor: Option<PathBuf>,
    pub submitter: Option<PathBuf>,
    pub receiver: PathBuf,
}

fn named(
    lookup: &impl Fn(&str) -> Option<OsString>,
    role: KeyRole,
) -> Result<Option<PathBuf>, KeyError> {
    match lookup(role.variable()) {
        None => Ok(None),
        Some(value) if value.is_empty() => Err(KeyError {
            role,
            refusal: KeyRefusal::Unset,
        }),
        Some(value) => Ok(Some(PathBuf::from(value))),
    }
}

impl KeyFiles {
    /// # Errors
    /// Refuses an unset receiver variable or any variable set to an empty value.
    pub fn from_env() -> Result<Self, KeyError> {
        Self::from_lookup(|name| std::env::var_os(name))
    }

    /// # Errors
    /// Refuses an unset receiver variable or any variable set to an empty value.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<OsString>) -> Result<Self, KeyError> {
        let attestor = named(&lookup, KeyRole::Attestor)?;
        let submitter = named(&lookup, KeyRole::Submitter)?;
        let receiver = named(&lookup, KeyRole::Receiver)?.ok_or(KeyError {
            role: KeyRole::Receiver,
            refusal: KeyRefusal::Unset,
        })?;
        Ok(Self {
            attestor,
            submitter,
            receiver,
        })
    }

    /// # Errors
    /// Refuses an unreadable, loosely permissioned, oversized or malformed key
    /// file, an invalid key, and two files that hold the same key.
    pub fn load(&self) -> Result<Keys, KeyError> {
        let attestor = self
            .attestor
            .as_deref()
            .map(|path| read_secret(path, KeyRole::Attestor))
            .transpose()?;
        let submitter = self
            .submitter
            .as_deref()
            .map(|path| read_secret(path, KeyRole::Submitter))
            .transpose()?;
        let receiver = read_secret(&self.receiver, KeyRole::Receiver)?;
        let loaded = [
            (KeyRole::Attestor, attestor.as_ref()),
            (KeyRole::Submitter, submitter.as_ref()),
            (KeyRole::Receiver, Some(&receiver)),
        ];
        for (index, (role, secret)) in loaded.iter().enumerate() {
            let Some(secret) = secret else { continue };
            for (earlier, other) in &loaded[..index] {
                if other.is_some_and(|other| other.as_slice() == secret.as_slice()) {
                    return Err(KeyError {
                        role: *role,
                        refusal: KeyRefusal::SameKeyAs(*earlier),
                    });
                }
            }
        }
        let secp = |secret: &Zeroizing<[u8; 32]>, role| {
            SecpKey::from_slice(secret.as_slice()).map_err(|_| KeyError {
                role,
                refusal: KeyRefusal::InvalidKey,
            })
        };
        let attestor = attestor
            .as_ref()
            .map(|secret| secp(secret, KeyRole::Attestor))
            .transpose()?;
        let submitter = submitter
            .as_ref()
            .map(|secret| secp(secret, KeyRole::Submitter))
            .transpose()?;
        if receiver.iter().all(|byte| *byte == 0) {
            return Err(KeyError {
                role: KeyRole::Receiver,
                refusal: KeyRefusal::InvalidKey,
            });
        }
        let receiver = ReceiverKey::from_bytes(&receiver);
        Ok(Keys {
            attestor,
            submitter,
            receiver,
        })
    }
}

fn read_secret(path: &Path, role: KeyRole) -> Result<Zeroizing<[u8; 32]>, KeyError> {
    let refuse = |refusal| KeyError { role, refusal };
    let file = std::fs::File::open(path).map_err(|_| refuse(KeyRefusal::Unreadable))?;
    let metadata = file
        .metadata()
        .map_err(|_| refuse(KeyRefusal::Unreadable))?;
    if !metadata.is_file() {
        return Err(refuse(KeyRefusal::Unreadable));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(refuse(KeyRefusal::Permissions));
        }
    }
    let mut text = Zeroizing::new(Vec::with_capacity(MAX_KEY_FILE_BYTES + 1));
    file.take(MAX_KEY_FILE_BYTES as u64 + 1)
        .read_to_end(&mut text)
        .map_err(|_| refuse(KeyRefusal::Unreadable))?;
    if text.len() > MAX_KEY_FILE_BYTES {
        return Err(refuse(KeyRefusal::Oversized));
    }
    let body = text
        .strip_suffix(b"\r\n")
        .or_else(|| text.strip_suffix(b"\n"))
        .unwrap_or(&text);
    let digits = body.strip_prefix(b"0x").unwrap_or(body);
    if digits.len() != 64 {
        return Err(refuse(KeyRefusal::Malformed));
    }
    let mut secret = Zeroizing::new([0_u8; 32]);
    for (byte, pair) in secret.iter_mut().zip(digits.chunks_exact(2)) {
        let (Some(high), Some(low)) = (nibble(pair[0]), nibble(pair[1])) else {
            return Err(refuse(KeyRefusal::Malformed));
        };
        *byte = (high << 4) | low;
    }
    Ok(secret)
}

const fn nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// The loaded keys. Formatting shows which keys are present and nothing else.
pub struct Keys {
    attestor: Option<SecpKey>,
    submitter: Option<SecpKey>,
    receiver: ReceiverKey,
}

impl Keys {
    #[must_use]
    pub const fn attestor(&self) -> Option<&SecpKey> {
        self.attestor.as_ref()
    }

    #[must_use]
    pub const fn submitter(&self) -> Option<&SecpKey> {
        self.submitter.as_ref()
    }

    #[must_use]
    pub const fn receiver(&self) -> &ReceiverKey {
        &self.receiver
    }
}

impl std::fmt::Debug for Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let presence = |present: bool| if present { "present" } else { "absent" };
        f.debug_struct("Keys")
            .field("attestor", &presence(self.attestor.is_some()))
            .field("submitter", &presence(self.submitter.is_some()))
            .field("receiver", &"present")
            .finish()
    }
}
