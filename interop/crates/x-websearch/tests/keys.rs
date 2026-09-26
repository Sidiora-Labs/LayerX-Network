use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use x_websearch::keys::{
    KeyError, KeyFiles, KeyRefusal, KeyRole, ATTESTOR_KEY_FILE, MAX_KEY_FILE_BYTES,
    RECEIVER_KEY_FILE, SUBMITTER_KEY_FILE,
};

const ATTESTOR_SECRET: &str = "0000000000000000000000000000000000000000000000000000000000000001";
const SUBMITTER_SECRET: &str = "0000000000000000000000000000000000000000000000000000000000000002";
const RECEIVER_SECRET: &str = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
const SECP_ORDER: &str = "fffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141";

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> std::io::Result<Self> {
        let path =
            std::env::temp_dir().join(format!("x-websearch-keys-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn write(&self, name: &str, contents: &[u8], mode: u32) -> std::io::Result<PathBuf> {
        let path = self.0.join(name);
        std::fs::write(&path, contents)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))?;
        Ok(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn lookup(entries: &[(&str, &Path)]) -> impl Fn(&str) -> Option<OsString> {
    let map: BTreeMap<String, OsString> = entries
        .iter()
        .map(|(name, path)| ((*name).to_owned(), path.as_os_str().to_owned()))
        .collect();
    move |name| map.get(name).cloned()
}

fn files(receiver: &Path) -> KeyFiles {
    KeyFiles {
        attestor: None,
        submitter: None,
        receiver: receiver.to_owned(),
    }
}

fn refused(role: KeyRole, refusal: KeyRefusal) -> KeyError {
    KeyError { role, refusal }
}

fn address(key: &k256::ecdsa::SigningKey) -> Vec<u8> {
    use sha3::Digest as _;
    let point = key.verifying_key().to_encoded_point(false);
    sha3::Keccak256::digest(&point.as_bytes()[1..])[12..].to_vec()
}

#[test]
fn all_three_keys_load_from_the_named_files() -> Result<(), Box<dyn std::error::Error>> {
    let scratch = Scratch::new("all")?;
    let attestor = scratch.write(
        "attestor.key",
        format!("{ATTESTOR_SECRET}\n").as_bytes(),
        0o600,
    )?;
    let submitter = scratch.write(
        "submitter.key",
        format!("0x{SUBMITTER_SECRET}\r\n").as_bytes(),
        0o400,
    )?;
    let receiver = scratch.write("receiver.key", RECEIVER_SECRET.as_bytes(), 0o600)?;
    let files = KeyFiles::from_lookup(lookup(&[
        (ATTESTOR_KEY_FILE, &attestor),
        (SUBMITTER_KEY_FILE, &submitter),
        (RECEIVER_KEY_FILE, &receiver),
    ]))?;
    assert_eq!(files.attestor.as_deref(), Some(attestor.as_path()));
    assert_eq!(files.submitter.as_deref(), Some(submitter.as_path()));
    assert_eq!(files.receiver, receiver);
    let keys = files.load()?;
    let attestor_key = keys.attestor().ok_or("attestor")?;
    assert_eq!(
        address(attestor_key),
        [
            0x7e, 0x5f, 0x45, 0x52, 0x09, 0x1a, 0x69, 0x12, 0x5d, 0x5d, 0xfc, 0xb7, 0xb8, 0xc2,
            0x65, 0x90, 0x29, 0x39, 0x5b, 0xdf
        ]
    );
    let submitter_key = keys.submitter().ok_or("submitter")?;
    assert_eq!(
        address(submitter_key),
        [
            0x2b, 0x5a, 0xd5, 0xc4, 0x79, 0x5c, 0x02, 0x65, 0x14, 0xf8, 0x31, 0x7c, 0x7a, 0x21,
            0x5e, 0x21, 0x8d, 0xcc, 0xd6, 0xcf
        ]
    );
    assert_eq!(
        keys.receiver().verifying_key().to_bytes(),
        [
            0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64,
            0x07, 0x3a, 0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68,
            0xf7, 0x07, 0x51, 0x1a
        ]
    );
    let debug = format!("{keys:?}");
    assert_eq!(
        debug,
        "Keys { attestor: \"present\", submitter: \"present\", receiver: \"present\" }"
    );
    Ok(())
}

#[test]
fn receiver_alone_is_enough_and_absent_roles_stay_absent() -> Result<(), Box<dyn std::error::Error>>
{
    let scratch = Scratch::new("receiver")?;
    let receiver = scratch.write("receiver.key", RECEIVER_SECRET.as_bytes(), 0o600)?;
    let files = KeyFiles::from_lookup(lookup(&[(RECEIVER_KEY_FILE, &receiver)]))?;
    assert_eq!(files, self::files(&receiver));
    let keys = files.load()?;
    assert!(keys.attestor().is_none());
    assert!(keys.submitter().is_none());
    assert_eq!(
        format!("{keys:?}"),
        "Keys { attestor: \"absent\", submitter: \"absent\", receiver: \"present\" }"
    );
    Ok(())
}

#[test]
fn unset_and_empty_variables_are_refused() {
    assert_eq!(
        KeyFiles::from_lookup(|_| None).err(),
        Some(refused(KeyRole::Receiver, KeyRefusal::Unset))
    );
    for (variable, role) in [
        (ATTESTOR_KEY_FILE, KeyRole::Attestor),
        (SUBMITTER_KEY_FILE, KeyRole::Submitter),
        (RECEIVER_KEY_FILE, KeyRole::Receiver),
    ] {
        let result = KeyFiles::from_lookup(|name| {
            if name == variable {
                Some(OsString::new())
            } else {
                Some(OsString::from("/nonexistent/receiver.key"))
            }
        });
        assert_eq!(
            result.err(),
            Some(refused(role, KeyRefusal::Unset)),
            "{variable}"
        );
    }
    assert_eq!(
        KeyError {
            role: KeyRole::Receiver,
            refusal: KeyRefusal::Unset
        }
        .to_string(),
        "key refused: X_WEBSEARCH_RECEIVER_KEY_FILE is not set"
    );
}

#[test]
fn unreadable_loose_oversized_and_malformed_files_are_refused(
) -> Result<(), Box<dyn std::error::Error>> {
    let scratch = Scratch::new("files")?;
    let missing = scratch.0.join("missing.key");
    let loose = scratch.write("loose.key", RECEIVER_SECRET.as_bytes(), 0o644)?;
    let group = scratch.write("group.key", RECEIVER_SECRET.as_bytes(), 0o640)?;
    let oversized = scratch.write("oversized.key", &vec![b'a'; MAX_KEY_FILE_BYTES + 1], 0o600)?;
    let short = scratch.write("short.key", &RECEIVER_SECRET.as_bytes()[..63], 0o600)?;
    let long = scratch.write("long.key", format!("{RECEIVER_SECRET}00").as_bytes(), 0o600)?;
    let non_hex = scratch.write(
        "non-hex.key",
        format!("{}zz", &RECEIVER_SECRET[..62]).as_bytes(),
        0o600,
    )?;
    let spaced = scratch.write(
        "spaced.key",
        format!(" {RECEIVER_SECRET}").as_bytes(),
        0o600,
    )?;
    let two_lines = scratch.write(
        "two-lines.key",
        format!("{RECEIVER_SECRET}\n\n").as_bytes(),
        0o600,
    )?;
    let zero = scratch.write("zero.key", "0".repeat(64).as_bytes(), 0o600)?;
    for (path, refusal) in [
        (missing, KeyRefusal::Unreadable),
        (scratch.0.clone(), KeyRefusal::Unreadable),
        (loose, KeyRefusal::Permissions),
        (group, KeyRefusal::Permissions),
        (oversized, KeyRefusal::Oversized),
        (short, KeyRefusal::Malformed),
        (long, KeyRefusal::Malformed),
        (non_hex, KeyRefusal::Malformed),
        (spaced, KeyRefusal::Malformed),
        (two_lines, KeyRefusal::Malformed),
        (zero, KeyRefusal::InvalidKey),
    ] {
        assert_eq!(
            files(&path).load().err(),
            Some(refused(KeyRole::Receiver, refusal)),
            "{}",
            path.display()
        );
    }
    Ok(())
}

#[test]
fn invalid_secp256k1_keys_are_refused() -> Result<(), Box<dyn std::error::Error>> {
    let scratch = Scratch::new("secp")?;
    let receiver = scratch.write("receiver.key", RECEIVER_SECRET.as_bytes(), 0o600)?;
    let zero = scratch.write("zero.key", "0".repeat(64).as_bytes(), 0o600)?;
    let order = scratch.write("order.key", SECP_ORDER.as_bytes(), 0o600)?;
    for path in [&zero, &order] {
        let attestor = KeyFiles {
            attestor: Some(path.clone()),
            submitter: None,
            receiver: receiver.clone(),
        };
        assert_eq!(
            attestor.load().err(),
            Some(refused(KeyRole::Attestor, KeyRefusal::InvalidKey))
        );
        let submitter = KeyFiles {
            attestor: None,
            submitter: Some(path.clone()),
            receiver: receiver.clone(),
        };
        assert_eq!(
            submitter.load().err(),
            Some(refused(KeyRole::Submitter, KeyRefusal::InvalidKey))
        );
    }
    Ok(())
}

#[test]
fn two_files_holding_the_same_key_are_refused() -> Result<(), Box<dyn std::error::Error>> {
    let scratch = Scratch::new("same")?;
    let attestor = scratch.write("attestor.key", ATTESTOR_SECRET.as_bytes(), 0o600)?;
    let submitter_copy = scratch.write(
        "submitter.key",
        format!("0x{}\n", ATTESTOR_SECRET.to_ascii_uppercase()).as_bytes(),
        0o600,
    )?;
    let receiver = scratch.write("receiver.key", RECEIVER_SECRET.as_bytes(), 0o600)?;
    let receiver_as_attestor =
        scratch.write("attestor-2.key", RECEIVER_SECRET.as_bytes(), 0o600)?;
    let receiver_copy = scratch.write("receiver-2.key", SUBMITTER_SECRET.as_bytes(), 0o600)?;
    let submitter = scratch.write("submitter-2.key", SUBMITTER_SECRET.as_bytes(), 0o600)?;
    for (files, role, other) in [
        (
            KeyFiles {
                attestor: Some(attestor.clone()),
                submitter: Some(submitter_copy),
                receiver: receiver.clone(),
            },
            KeyRole::Submitter,
            KeyRole::Attestor,
        ),
        (
            KeyFiles {
                attestor: Some(attestor.clone()),
                submitter: Some(attestor.clone()),
                receiver: receiver.clone(),
            },
            KeyRole::Submitter,
            KeyRole::Attestor,
        ),
        (
            KeyFiles {
                attestor: Some(receiver_as_attestor),
                submitter: None,
                receiver: receiver.clone(),
            },
            KeyRole::Receiver,
            KeyRole::Attestor,
        ),
        (
            KeyFiles {
                attestor: None,
                submitter: Some(submitter),
                receiver: receiver_copy,
            },
            KeyRole::Receiver,
            KeyRole::Submitter,
        ),
    ] {
        assert_eq!(
            files.load().err(),
            Some(refused(role, KeyRefusal::SameKeyAs(other)))
        );
    }
    Ok(())
}

#[test]
fn refusals_never_format_key_material() -> Result<(), Box<dyn std::error::Error>> {
    let scratch = Scratch::new("format")?;
    let loose = scratch.write("loose.key", RECEIVER_SECRET.as_bytes(), 0o644)?;
    let error = files(&loose).load().err().ok_or("accepted")?;
    for text in [error.to_string(), format!("{error:?}")] {
        assert!(!text.contains(RECEIVER_SECRET));
        assert!(!text.contains(&loose.display().to_string()));
    }
    assert_eq!(
        error.to_string(),
        "key refused: the file X_WEBSEARCH_RECEIVER_KEY_FILE names is readable by group or others"
    );
    let same = KeyError {
        role: KeyRole::Submitter,
        refusal: KeyRefusal::SameKeyAs(KeyRole::Attestor),
    };
    assert_eq!(
        same.to_string(),
        "key refused: the file X_WEBSEARCH_SUBMITTER_KEY_FILE names holds the same key as X_WEBSEARCH_ATTESTOR_KEY_FILE"
    );
    let receiver = scratch.write("receiver.key", RECEIVER_SECRET.as_bytes(), 0o600)?;
    let keys = files(&receiver).load()?;
    let debug = format!("{keys:?}");
    assert!(!debug.contains(RECEIVER_SECRET));
    assert!(!debug.to_ascii_lowercase().contains("9d61b1"));
    Ok(())
}
