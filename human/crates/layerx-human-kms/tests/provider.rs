use layerx_client::lni::framing::{read_frame, write_frame};
use layerx_client::lni::transport::{Limits, MutualTlsConfig};
use layerx_human_service::custody::{KeyClass, KeyId, Keystore, KmsProvider, RemoteKmsProvider};
use layerx_human_service::store::PrincipalId;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use std::error::Error;
use std::fs;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const MAX: usize = 2_097_152;
struct Host {
    root: PathBuf,
    address: SocketAddr,
    child: Option<Child>,
}
impl Drop for Host {
    fn drop(&mut self) {
        self.stop();
        let _ = fs::remove_dir_all(&self.root);
    }
}
impl Host {
    fn new() -> Result<Self> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let root = std::env::temp_dir().join(format!(
            "lxkp-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        fs::create_dir(&root)?;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        drop(listener);
        let mut host = Self {
            root,
            address,
            child: None,
        };
        host.openssl(&[
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
            "/CN=LXKP test CA",
            "-addext",
            "basicConstraints=critical,CA:TRUE",
        ])?;
        for name in ["server", "client", "foreign"] {
            host.openssl(&[
                "req",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-keyout",
                &format!("{name}.key"),
                "-out",
                &format!("{name}.csr"),
                "-subj",
                &format!("/CN={name}"),
            ])?;
            fs::write(
                host.root.join("extensions"),
                if name == "server" {
                    "subjectAltName=DNS:localhost\nextendedKeyUsage=serverAuth\n"
                } else {
                    "extendedKeyUsage=clientAuth\n"
                },
            )?;
            host.openssl(&[
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
                "extensions",
            ])?;
            host.openssl(&[
                "x509",
                "-in",
                &format!("{name}.pem"),
                "-outform",
                "DER",
                "-out",
                &format!("{name}.der"),
            ])?;
            host.openssl(&[
                "pkcs8",
                "-topk8",
                "-nocrypt",
                "-in",
                &format!("{name}.key"),
                "-outform",
                "DER",
                "-out",
                &format!("{name}-key.der"),
            ])?;
        }
        host.openssl(&["x509", "-in", "ca.pem", "-outform", "DER", "-out", "ca.der"])?;
        let mut seal = [0; 32];
        getrandom::fill(&mut seal)?;
        fs::write(host.root.join("seal"), seal)?;
        let kind =
            checked(layerx_types::payload::ActivityType::new(layerx_types::payload::ModuleId::Asset, 5))?;
        fs::write(
            host.root.join("registry.json"),
            serde_json::to_vec(
                &serde_json::json!({"network_id":77,"protocol_version":3,"modules":[{"module_id":layerx_types::payload::ModuleId::Asset as u16,"activity_types":[kind.value()]}]}),
            )?,
        )?;
        for entry in fs::read_dir(&host.root)? {
            fs::set_permissions(entry?.path(), fs::Permissions::from_mode(0o600))?;
        }
        host.start()?;
        Ok(host)
    }
    fn openssl(&self, arguments: &[&str]) -> Result<()> {
        let result = Command::new("openssl")
            .args(arguments)
            .current_dir(&self.root)
            .output()?;
        if !result.status.success() {
            return Err(format!(
                "openssl failed: {}",
                String::from_utf8_lossy(&result.stderr)
            )
            .into());
        }
        Ok(())
    }
    fn launch(&self) -> Result<Child> {
        let mut command = Command::new(env!("CARGO_BIN_EXE_layerx-human-kms"));
        command
            .env("LAYERX_HUMAN_KMS_LISTEN", self.address.to_string())
            .env("LAYERX_HUMAN_KMS_PROVIDER_REFERENCE", "beta-kms")
            .env("LAYERX_HUMAN_KMS_STATE_DIR", self.root.join("state"))
            .env("LAYERX_HUMAN_KMS_DEADLINE_SECONDS", "2");
        for (suffix, file) in [
            ("REGISTRY_FILE", "registry.json"),
            ("CLIENT_CA_DER", "ca.der"),
            ("TLS_CERT_DER", "server.der"),
            ("TLS_KEY_DER", "server-key.der"),
            ("CLIENT_CERT_DER", "client.der"),
            ("SEAL_SECRET_FILE", "seal"),
        ] {
            command.env(format!("LAYERX_HUMAN_KMS_{suffix}"), self.root.join(file));
        }
        Ok(command
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?)
    }
    fn start(&mut self) -> Result<()> {
        self.child = Some(self.launch()?);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if self.remote("client", "beta-kms")?.probe().is_ok() {
                return Ok(());
            }
            if self
                .child
                .as_mut()
                .ok_or("missing process")?
                .try_wait()?
                .is_some()
            {
                return Err("KMS exited at startup".into());
            }
            if Instant::now() >= deadline {
                return Err("KMS startup deadline".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
    fn roots(&self) -> Result<RootCertStore> {
        let mut roots = RootCertStore::empty();
        roots.add(CertificateDer::from(fs::read(self.root.join("ca.der"))?))?;
        Ok(roots)
    }
    fn identity(
        &self,
        name: &str,
    ) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
        Ok((
            vec![CertificateDer::from(fs::read(
                self.root.join(format!("{name}.der")),
            )?)],
            PrivateKeyDer::try_from(fs::read(self.root.join(format!("{name}-key.der")))?)?,
        ))
    }
    fn remote(&self, name: &str, provider: &str) -> Result<RemoteKmsProvider> {
        let (cert, key) = self.identity(name)?;
        Ok(RemoteKmsProvider::new(
            provider,
            self.address,
            "localhost",
            checked(MutualTlsConfig::new(self.roots()?, cert, key))?,
            Limits {
                maximum_frame_bytes: MAX,
                maximum_connections: 4,
                maximum_streams: 1,
                maximum_queued_bytes: MAX,
                deadline: Duration::from_secs(2),
            },
        )?)
    }
    fn connection(&self, name: Option<&str>) -> Result<StreamOwned<ClientConnection, TcpStream>> {
        let builder = ClientConfig::builder().with_root_certificates(self.roots()?);
        let config = if let Some(name) = name {
            let (cert, key) = self.identity(name)?;
            builder.with_client_auth_cert(cert, key)?
        } else {
            builder.with_no_client_auth()
        };
        let tcp = TcpStream::connect(self.address)?;
        tcp.set_read_timeout(Some(Duration::from_secs(3)))?;
        tcp.set_write_timeout(Some(Duration::from_secs(3)))?;
        Ok(StreamOwned::new(
            ClientConnection::new(Arc::new(config), ServerName::try_from("localhost")?)?,
            tcp,
        ))
    }
    fn call(&self, request: &[u8]) -> Result<Vec<u8>> {
        let mut tls = self.connection(Some("client"))?;
        checked(write_frame(&mut tls, request, MAX))?;
        checked(read_frame(&mut tls, MAX))
    }
}
fn blob(out: &mut Vec<u8>, value: &[u8]) -> Result<()> {
    out.extend(u32::try_from(value.len())?.to_be_bytes());
    out.extend(value);
    Ok(())
}
fn request(
    op: u8,
    binding: [u8; 32],
    reference: &[u8],
    expected: Option<[u8; 32]>,
) -> Result<Vec<u8>> {
    let mut out = b"LXKP".to_vec();
    out.extend(if expected.is_some() { 2_u16 } else { 1_u16 }.to_be_bytes());
    out.push(op);
    blob(&mut out, b"beta-kms")?;
    if op != 0 {
        out.extend(binding);
        out.extend(77_u32.to_be_bytes());
        out.push(1);
        blob(&mut out, reference)?;
    }
    if let Some(expected) = expected {
        out.extend(expected);
    }
    Ok(out)
}
fn facts(bytes: &[u8]) -> Result<([u8; 32], [u8; 32])> {
    assert_eq!(&bytes[..4], b"LXKP");
    assert_eq!(bytes[7], 0);
    assert_eq!(bytes.len(), 109);
    assert_eq!(&bytes[8..12], &32_u32.to_be_bytes());
    Ok((bytes[12..44].try_into()?, bytes[44..76].try_into()?))
}

#[test]
fn actual_client_lifecycle_and_mutual_tls() -> Result<()> {
    let mut host = Host::new()?;
    let alice = PrincipalId::new("alice")?;
    let bob = PrincipalId::new("bob")?;
    let key = KeyId::new("primary")?;
    let store = Keystore::open_production(
        host.root.join("client-state"),
        77,
        host.remote("client", "beta-kms")?,
    )?;
    let original = store.create(&alice, &key, KeyClass::HumanPrimary)?;
    assert_eq!(store.describe(&alice, &key)?.public_key, original);
    assert!(store.describe(&bob, &key).is_err());
    let bob_public = store.create(&bob, &key, KeyClass::AgentPrimary)?;
    assert_ne!(original, bob_public);
    assert_eq!(store.describe(&bob, &key)?.public_key, bob_public);
    let rotated = store.rotate(&alice, &key)?.public_key;
    assert_ne!(original, rotated);
    let next = store.rotate(&alice, &key)?.public_key;
    assert_ne!(rotated, next);
    host.stop();
    host.start()?;
    assert_eq!(store.describe(&alice, &key)?.public_key, next);
    assert!(host.remote("foreign", "beta-kms")?.probe().is_err());
    assert!(host
        .remote("client", "different-provider")?
        .probe()
        .is_err());
    let mut unauthenticated = host.connection(None)?;
    let probe = request(0, [0; 32], &[], None)?;
    let no_cert = write_frame(&mut unauthenticated, &probe, MAX)
        .and_then(|()| read_frame(&mut unauthenticated, MAX));
    assert!(no_cert.is_err());
    store.destroy(&alice, &key)?;
    assert_eq!(store.describe(&bob, &key)?.public_key, bob_public);
    store.destroy(&bob, &key)?;
    assert!(store.describe(&alice, &key).is_err());
    assert!(store.create(&alice, &key, KeyClass::HumanPrimary).is_err());
    Ok(())
}

#[test]
fn atomic_rotation_lost_response_restart_and_tombstones() -> Result<()> {
    let mut host = Host::new()?;
    let binding = [31; 32];
    let create = request(1, binding, &[], None)?;
    let first = host.call(&create)?;
    assert_eq!(first, host.call(&create)?);
    let (handle, original) = facts(&first)?;
    let rotate = request(3, binding, &handle, Some(original))?;
    {
        let mut tls = host.connection(Some("client"))?;
        checked(write_frame(&mut tls, &rotate, MAX))?;
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let (_, observed) = facts(&host.call(&request(2, binding, &handle, None)?)?)?;
        if observed != original { break; }
        if Instant::now() >= deadline { return Err("lost-response rotation was not committed".into()); }
        std::thread::sleep(Duration::from_millis(20));
    }
    let committed = host.call(&rotate)?;
    let (same, next) = facts(&committed)?;
    assert_eq!(same, handle);
    assert_ne!(original, next);
    host.stop();
    host.start()?;
    assert_eq!(committed, host.call(&rotate)?);
    let second = host.call(&request(3, binding, &handle, Some(next))?)?;
    let (_, latest) = facts(&second)?;
    assert_ne!(latest, next);
    assert_eq!(host.call(&rotate)?[7], 3);
    assert_eq!(host.call(&request(3, binding, &handle, None)?)?[7], 1);
    assert_eq!(host.call(&request(2, [32; 32], &handle, None)?)?[7], 2);
    assert_eq!(host.call(&request(2, binding, &[33; 32], None)?)?[7], 5);
    let mut wrong_class = request(2, binding, &handle, None)?;
    wrong_class[55] = 2;
    assert_eq!(host.call(&wrong_class)?[7], 5);
    let mut wrong_network = request(2, binding, &handle, None)?;
    wrong_network[54] ^= 1;
    assert_eq!(host.call(&wrong_network)?[7], 1);
    let destroy = request(4, binding, &handle, None)?;
    assert_eq!(host.call(&destroy)?[7], 0);
    assert_eq!(host.call(&destroy)?[7], 0);
    host.stop();
    host.start()?;
    assert_eq!(host.call(&destroy)?[7], 0);
    assert_eq!(host.call(&create)?[7], 3);
    assert_eq!(host.call(&request(2, binding, &handle, None)?)?[7], 2);
    host.stop();
    let path = host.root.join("state/state.aead");
    let mut encrypted = fs::read(&path)?;
    encrypted[20] ^= 1;
    fs::write(path, encrypted)?;
    let mut child = host.launch()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait()? {
            assert!(!status.success());
            break;
        }
        if Instant::now() >= deadline {
            child.kill()?;
            child.wait()?;
            return Err("tampered state did not fail closed".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

fn canonical(public: [u8; 32], network: u32) -> Result<(Vec<u8>, Vec<u8>)> {
    use layerx_types::account::AccountId;
    use layerx_types::activity::{Authority, EnvelopeBuilder, TimestampBound};
    use layerx_types::amount::Amount;
    use layerx_types::ids::{AssetId, Did, IdempotencyKey};
    use layerx_types::intent::{
        AuthorizationSignature, ContextHash, NetworkId, ProtocolVersion, PublicKey,
        SendAuthorization, SendAuthorizationKind, Sequence, TimestampSeconds,
    };
    use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
    let kind = checked(ActivityType::new(ModuleId::Asset, 5))?;
    let registry = checked(ModuleRegistry::new(&[checked(ModuleRegistration::new(
        ModuleId::Asset,
        &[kind],
    ))?]))?;
    let send = checked(layerx_intents::LxpSend::new(
        checked(AccountId::parse("agent:did:layerx:alice:main"))?,
        checked(AccountId::parse("agent:did:layerx:recipient:main"))?,
        AssetId::new([3; 32]),
        Amount::from_u128(10),
        Sequence::from_u64(7),
        IdempotencyKey::new([4; 32]),
        TimestampSeconds::from_u64(1010),
        ContextHash::new([5; 32]),
        SendAuthorization::new(
            SendAuthorizationKind::Owner,
            PublicKey::new(public),
            AuthorizationSignature::new([6; 64]),
        ),
        checked(NetworkId::new(network))?,
        checked(ProtocolVersion::new(3))?,
    ))?;
    let compiled = checked(layerx_intents::compile(
        &layerx_intents::Intent::v1(layerx_intents::IntentKind::LxpSend(send)),
        &registry,
    ))?;
    let mut builder = EnvelopeBuilder::new();
    checked(builder.protocol_version(3))?;
    checked(builder.network_id(network))?;
    checked(builder.activity_type(kind))?;
    checked(builder.actor_did(checked(Did::new(b"did:layerx:alice"))?))?;
    checked(builder.authority(checked(Authority::owner(&public))?))?;
    checked(builder.account_sequence(7))?;
    checked(builder.timestamp_bound(checked(TimestampBound::new(1000, 1010))?))?;
    checked(builder.idempotency_key(IdempotencyKey::new([4; 32])))?;
    checked(builder.fee_limit(Amount::from_u128(1)))?;
    checked(builder.payload_hash(compiled.payload_hash()))?;
    checked(builder.payload(compiled.payload().clone()))?;
    let bytes = checked(layerx_wire::activity::encode_unsigned_envelope(&checked(
        builder.build(),
    )?))?;
    let disclosure = checked(layerx_crypto::disclosure::bind(&bytes, &registry))?;
    let mut out = vec![1];
    out.extend(disclosure.activity_type.value().to_be_bytes());
    blob(&mut out, &disclosure.actor)?;
    blob(&mut out, &disclosure.authority)?;
    out.extend(u32::try_from(disclosure.counterparties.len())?.to_be_bytes());
    for party in &disclosure.counterparties {
        out.push(match party.role {
            layerx_crypto::disclosure::CounterpartyRole::Payer => 1,
            layerx_crypto::disclosure::CounterpartyRole::Recipient => 2,
        });
        out.extend(party.account);
    }
    out.extend(u32::try_from(disclosure.amounts.len())?.to_be_bytes());
    for amount in &disclosure.amounts {
        out.push(match amount.role {
            layerx_crypto::disclosure::AmountRole::Transfer => 1,
            layerx_crypto::disclosure::AmountRole::SpendingLimit => 2,
        });
        out.extend(amount.value.to_be_bytes());
    }
    out.extend(disclosure.asset);
    out.extend(disclosure.fee_limit.to_be_bytes());
    out.extend(disclosure.expiry.not_before.to_be_bytes());
    out.extend(disclosure.expiry.not_after.to_be_bytes());
    out.extend(disclosure.expiry.payload_expires_at.to_be_bytes());
    out.extend(disclosure.idempotency_key);
    assert!(disclosure.evm_payout_binding.is_none());
    out.push(0);
    Ok((bytes, out))
}
fn checked<T, E: std::fmt::Debug>(value: std::result::Result<T, E>) -> Result<T> {
    value.map_err(|error| format!("{error:?}").into())
}
fn signing_request(
    binding: [u8; 32],
    handle: &[u8],
    canonical: &[u8],
    disclosure: &[u8],
) -> Result<(Vec<u8>, [u8; 32])> {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(b"LXP/v1/signature-preimage\0");
    hash.update(canonical);
    let digest: [u8; 32] = hash.finalize().into();
    let mut bytes = request(5, binding, handle, None)?;
    bytes.extend(digest);
    blob(&mut bytes, canonical)?;
    blob(&mut bytes, disclosure)?;
    Ok((bytes, digest))
}
#[test]
fn canonical_signing_and_disclosure_refusals() -> Result<()> {
    let host = Host::new()?;
    let binding = [41; 32];
    let (handle, public) = facts(&host.call(&request(1, binding, &[], None)?)?)?;
    let (canonical, disclosure) = canonical(public, 77)?;
    let (bytes, digest) = signing_request(binding, &handle, &canonical, &disclosure)?;
    let response = host.call(&bytes)?;
    assert_eq!(response[7], 0);
    assert_eq!(response.len(), 72);
    checked(
        ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, public)
            .verify(&digest, &response[8..]),
    )?;
    let mut changed_disclosure = disclosure.clone();
    let last = changed_disclosure.len() - 2;
    changed_disclosure[last] ^= 1;
    assert_eq!(
        host.call(&signing_request(binding, &handle, &canonical, &changed_disclosure)?.0)?[7],
        1
    );
    let mut changed_digest = bytes.clone();
    changed_digest[92] ^= 1;
    assert_eq!(host.call(&changed_digest)?[7], 5);
    let (foreign, foreign_disclosure) = self::canonical(public, 78)?;
    assert_eq!(
        host.call(&signing_request(binding, &handle, &foreign, &foreign_disclosure)?.0)?[7],
        1
    );
    let mut noncanonical = canonical;
    noncanonical.push(0);
    assert_eq!(
        host.call(&signing_request(binding, &handle, &noncanonical, &disclosure)?.0)?[7],
        1
    );
    let mut trailing = bytes;
    trailing.push(0);
    assert!(host.call(&trailing).is_err());
    let mut tls = host.connection(Some("client"))?;
    use std::io::Write;
    tls.write_all(&u32::try_from(MAX + 1)?.to_be_bytes())?;
    assert!(read_frame(&mut tls, MAX).is_err());
    Ok(())
}
