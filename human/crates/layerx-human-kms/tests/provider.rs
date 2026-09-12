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
        host.provision_tls()?;
        let mut seal = [0; 32];
        getrandom::fill(&mut seal).map_err(|error| std::io::Error::other(error.to_string()))?;
        fs::write(host.root.join("seal"), seal)?;
        let modules: Vec<_> = registry()?
            .registrations()
            .iter()
            .map(|module| {
                let kinds: Vec<_> = module
                    .activity_types()
                    .iter()
                    .map(|kind| kind.value())
                    .collect();
                serde_json::json!({"module_id":module.module() as u16,"activity_types":kinds})
            })
            .collect();
        fs::write(
            host.root.join("registry.json"),
            serde_json::to_vec(
                &serde_json::json!({"network_id":77,"protocol_version":3,"modules":modules}),
            )?,
        )?;
        for entry in fs::read_dir(&host.root)? {
            fs::set_permissions(entry?.path(), fs::Permissions::from_mode(0o600))?;
        }
        host.start()?;
        Ok(host)
    }
    fn provision_tls(&self) -> Result<()> {
        self.openssl(&[
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
            self.openssl(&[
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
                self.root.join("extensions"),
                if name == "server" {
                    "subjectAltName=DNS:localhost\nextendedKeyUsage=serverAuth\n"
                } else {
                    "extendedKeyUsage=clientAuth\n"
                },
            )?;
            self.openssl(&[
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
            self.openssl(&[
                "x509",
                "-in",
                &format!("{name}.pem"),
                "-outform",
                "DER",
                "-out",
                &format!("{name}.der"),
            ])?;
            self.openssl(&[
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
        self.openssl(&["x509", "-in", "ca.pem", "-outform", "DER", "-out", "ca.der"])?;
        Ok(())
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
            .env("LAYERX_HUMAN_KMS_DEADLINE_SECONDS", "2")
            .env(
                "LAYERX_HUMAN_KMS_EVM_CLIENT_CERT_DER",
                self.root.join("foreign.der"),
            );
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
        if observed != original {
            break;
        }
        if Instant::now() >= deadline {
            return Err("lost-response rotation was not committed".into());
        }
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

fn registry() -> Result<layerx_types::payload::ModuleRegistry> {
    use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
    checked(ModuleRegistry::new(&[
        checked(ModuleRegistration::new(
            ModuleId::Asset,
            &[
                checked(ActivityType::new(ModuleId::Asset, 1))?,
                checked(ActivityType::new(ModuleId::Asset, 5))?,
            ],
        ))?,
        checked(ModuleRegistration::new(
            ModuleId::Governance,
            &[checked(ActivityType::new(ModuleId::Governance, 8))?],
        ))?,
    ]))
}

fn canonical_send(public: [u8; 32], network: u32, signature: [u8; 64]) -> Result<Vec<u8>> {
    use layerx_types::account::AccountId;
    use layerx_types::amount::Amount;
    use layerx_types::ids::{AssetId, IdempotencyKey};
    use layerx_types::intent::{
        AuthorizationSignature, ContextHash, NetworkId, ProtocolVersion, PublicKey,
        SendAuthorization, SendAuthorizationKind, Sequence, TimestampSeconds,
    };
    let registry = registry()?;
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
            AuthorizationSignature::new(signature),
        ),
        checked(NetworkId::new(network))?,
        checked(ProtocolVersion::new(3))?,
    ))?;
    let compiled = checked(layerx_intents::compile(
        &layerx_intents::Intent::v1(layerx_intents::IntentKind::LxpSend(send)),
        &registry,
    ))?;
    unsigned_payload(public, network, compiled.payload().clone())
}

fn unsigned_payload(
    public: [u8; 32],
    network: u32,
    payload: layerx_types::payload::Payload,
) -> Result<Vec<u8>> {
    use layerx_types::activity::{Authority, EnvelopeBuilder, TimestampBound};
    use layerx_types::amount::Amount;
    use layerx_types::ids::{Did, IdempotencyKey};
    let mut builder = EnvelopeBuilder::new();
    checked(builder.protocol_version(3))?;
    checked(builder.network_id(network))?;
    checked(builder.activity_type(payload.activity_type()))?;
    checked(builder.actor_did(checked(Did::new(b"did:layerx:alice"))?))?;
    checked(builder.authority(checked(Authority::owner(&public))?))?;
    checked(builder.account_sequence(7))?;
    checked(builder.timestamp_bound(checked(TimestampBound::new(1000, 1010))?))?;
    checked(builder.idempotency_key(IdempotencyKey::new([4; 32])))?;
    checked(builder.fee_limit(Amount::from_u128(1)))?;
    checked(builder.payload_hash(checked(layerx_wire::hash::payload_hash_for(&payload))?))?;
    checked(builder.payload(payload))?;
    checked(layerx_wire::activity::encode_unsigned_envelope(&checked(
        builder.build(),
    )?))
}

fn encoded_disclosure(disclosure: &layerx_crypto::disclosure::Disclosure) -> Result<Vec<u8>> {
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
            layerx_crypto::disclosure::AmountRole::SupplyCap => 3,
            layerx_crypto::disclosure::AmountRole::PerDrawMaximum => 4,
            layerx_crypto::disclosure::AmountRole::GrantAllowance => 5,
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
    Ok(out)
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
fn authorize_canonical_send(host: &Host, binding: [u8; 32], handle: &[u8]) -> Result<[u8; 64]> {
    use layerx_human_service::custody::SendPlanAuthorization;
    use layerx_types::account::AccountId;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let authorization = SendPlanAuthorization {
        plan_id: [61; 32],
        action_key: [62; 32],
        principal: "alice".into(),
        tenant: "tenant".into(),
        binding_digest: binding,
        from: checked(layerx_wire::hash::account_id_for_protocol(
            &checked(AccountId::parse("agent:did:layerx:alice:main"))?,
            3,
        ))?,
        to: checked(layerx_wire::hash::account_id_for_protocol(
            &checked(AccountId::parse("agent:did:layerx:recipient:main"))?,
            3,
        ))?,
        asset: [3; 32],
        amount: 10,
        sequence: 7,
        idempotency_key: [4; 32],
        expires_at: 1010,
        context: [5; 32],
        network: 77,
        protocol: 3,
        not_before: now,
        not_after: now + 600,
    };
    let mut bytes = request(11, binding, handle, None)?;
    bytes[4..6].copy_from_slice(&3_u16.to_be_bytes());
    blob(&mut bytes, &serde_json::to_vec(&authorization)?)?;
    let response = host.call(&bytes)?;
    assert_eq!(response[7], 0);
    assert_eq!(response.len(), 72);
    Ok(response[8..].try_into()?)
}

#[test]
fn canonical_signing_and_disclosure_refusals() -> Result<()> {
    use std::io::Write;
    let host = Host::new()?;
    let binding = [41; 32];
    let (handle, public) = facts(&host.call(&request(1, binding, &[], None)?)?)?;
    let signature = authorize_canonical_send(&host, binding, &handle)?;
    let canonical = canonical_send(public, 77, signature)?;
    let disclosure = encoded_disclosure(&checked(layerx_crypto::disclosure::bind(
        &canonical,
        &registry()?,
    ))?)?;
    let forged = canonical_send(public, 77, [6; 64])?;
    assert!(matches!(
        layerx_crypto::disclosure::bind(&forged, &registry()?),
        Err(layerx_crypto::disclosure::DisclosureError::MalformedPayload)
    ));
    assert_eq!(
        host.call(&signing_request(binding, &handle, &forged, &disclosure)?.0)?[7],
        1
    );
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
    let foreign = canonical_send(public, 78, signature)?;
    assert_eq!(
        host.call(&signing_request(binding, &handle, &foreign, &disclosure)?.0)?[7],
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
    tls.write_all(&u32::try_from(MAX + 1)?.to_be_bytes())?;
    assert!(read_frame(&mut tls, MAX).is_err());
    Ok(())
}

fn monetary_payloads() -> Result<Vec<layerx_types::payload::Payload>> {
    use layerx_crypto::authority_grant::AuthorityGrant;
    use layerx_crypto::payments::{asset_id, Payment, Registration};
    use layerx_types::ids::Did;
    use layerx_types::payload::{ActivityType, ModuleId, Payload};
    let actor = checked(Did::new(b"did:layerx:alice"))?;
    let issuer = checked(layerx_wire::hash::did_id_for_protocol(&actor, 3))?;
    let registration = Payment::Register(Registration {
        asset: asset_id(&issuer, &[21; 32]),
        salt: [21; 32],
        symbol: "KMS".into(),
        name: "KMS asset".into(),
        decimals: 6,
        supply_cap: 1000,
        issuer_kind: 1,
        custody_ref: Vec::new(),
    });
    let mut payloads = vec![checked(Payload::new(
        &registry()?,
        checked(ActivityType::new(ModuleId::Asset, 1))?,
        &checked(registration.encode(actor.as_bytes()))?,
    ))?];
    for bytes in [
        include_bytes!(
            "../../../../agent/crates/layerx-crypto/tests/fixtures/authority-grant-capability.bin"
        )
        .as_slice(),
        include_bytes!(
            "../../../../agent/crates/layerx-crypto/tests/fixtures/authority-grant-budget.bin"
        )
        .as_slice(),
    ] {
        let mut grant = checked(AuthorityGrant::decode(bytes))?;
        grant.grantor = issuer;
        grant.grantee = issuer;
        payloads.push(checked(Payload::new(
            &registry()?,
            checked(ActivityType::new(ModuleId::Governance, 8))?,
            &checked(grant.payload())?,
        ))?);
    }
    Ok(payloads)
}

#[test]
fn monetary_roles_are_bound_by_the_real_provider_before_and_after_restart() -> Result<()> {
    use layerx_crypto::disclosure::{bind, AmountRole};
    let mut host = Host::new()?;
    let binding = [71; 32];
    let (handle, public) = facts(&host.call(&request(1, binding, &[], None)?)?)?;
    let payloads = monetary_payloads()?;
    for pass in 0..2 {
        if pass == 1 {
            host.stop();
            host.start()?;
        }
        for payload in &payloads {
            let canonical = unsigned_payload(public, 77, payload.clone())?;
            let disclosure = checked(bind(&canonical, &registry()?))?;
            let roles: Vec<_> = disclosure
                .amounts
                .iter()
                .map(|amount| amount.role)
                .collect();
            if disclosure.payment.is_some() {
                assert_eq!(roles, [AmountRole::SupplyCap]);
                assert_eq!(disclosure.amounts[0].value, 1000);
            } else {
                assert_eq!(
                    roles,
                    [
                        AmountRole::PerDrawMaximum,
                        AmountRole::GrantAllowance,
                        AmountRole::SpendingLimit
                    ]
                );
                assert_eq!(disclosure.amounts[0].value, 10);
                assert_eq!(disclosure.amounts[1].value, 30);
            }
            let encoded = encoded_disclosure(&disclosure)?;
            let (request, digest) = signing_request(binding, &handle, &canonical, &encoded)?;
            let response = host.call(&request)?;
            assert_eq!(response[7], 0);
            assert_eq!(response.len(), 72);
            checked(
                ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, public)
                    .verify(&digest, &response[8..]),
            )?;
            let start = 1
                + 4
                + 4
                + disclosure.actor.len()
                + 4
                + disclosure.authority.len()
                + 4
                + 33 * disclosure.counterparties.len()
                + 4;
            let expected: &[u8] = if disclosure.payment.is_some() {
                &[3]
            } else {
                &[4, 5, 2]
            };
            for (index, code) in expected.iter().enumerate() {
                let offset = start + index * 17;
                assert_eq!(encoded[offset], *code);
                for mutation in [offset, offset + 16] {
                    let mut changed = encoded.clone();
                    changed[mutation] ^= 0x80;
                    assert_eq!(
                        host.call(&signing_request(binding, &handle, &canonical, &changed)?.0)?[7],
                        1
                    );
                }
            }
        }
    }
    Ok(())
}

#[test]
fn evm_authorization_nonce_dedup_and_acknowledgement_recovery() -> Result<()> {
    use layerx_human_service::custody::{EvmAcknowledgement, EvmPlanAuthorization, EvmTransaction};
    let mut host = Host::new()?;
    let principal = PrincipalId::new("alice")?;
    let key = KeyId::new("primary")?;
    let store = Keystore::open_production(
        host.root.join("evm-client"),
        77,
        host.remote("client", "beta-kms")?,
    )?;
    store.create(&principal, &key, KeyClass::HumanPrimary)?;
    let wallet = store.evm_wallet(&principal, &key)?;
    assert_ne!(wallet, [0; 20]);
    assert_eq!(wallet, store.evm_wallet(&principal, &key)?);
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let authorization = EvmPlanAuthorization {
        plan_id: [1; 32],
        action_key: [2; 32],
        tenant: "tenant".into(),
        principal: "alice".into(),
        binding_digest: store.evm_binding(&principal, &key)?.digest(),
        wallet,
        not_before: now,
        not_after: now + 600,
        transaction: EvmTransaction {
            chain_id: 31337,
            nonce: 7,
            max_priority_fee_per_gas: 1,
            max_fee_per_gas: 2,
            gas_limit: 21000,
            to: [3; 20],
            value: [0; 32],
            calldata: vec![],
        },
    };
    assert!(store
        .sign_evm_action(&principal, &key, &authorization.action_key)
        .is_err());
    let reserved = store.authorize_evm_plan(&principal, &key, &authorization)?;
    refuse_cross_scope(&store, &principal, &key, &authorization)?;
    assert!(reserved.raw_transaction.is_empty());
    assert_eq!(
        reserved,
        store.authorize_evm_plan(&principal, &key, &authorization)?
    );
    let mut conflict = authorization.clone();
    conflict.action_key = [4; 32];
    assert!(store
        .authorize_evm_plan(&principal, &key, &conflict)
        .is_err());
    conflict = authorization.clone();
    conflict.transaction.value = [5; 32];
    assert!(store
        .authorize_evm_plan(&principal, &key, &conflict)
        .is_err());
    let signed = store.sign_evm_action(&principal, &key, &authorization.action_key)?;
    assert_eq!(signed.raw_transaction[0], 2);
    assert_eq!(
        signed,
        store.sign_evm_action(&principal, &key, &authorization.action_key)?
    );
    host.stop();
    host.start()?;
    assert_eq!(
        signed,
        store.recover_evm_action(&principal, &key, &authorization.action_key)?
    );
    assert_eq!(
        signed,
        store.sign_evm_action(&principal, &key, &authorization.action_key)?
    );
    assert!(!signed.acknowledged);
    let mut ack = EvmAcknowledgement {
        action_key: authorization.action_key,
        transaction_hash: [0; 32],
    };
    assert!(store
        .acknowledge_evm_action(&principal, &key, &ack)
        .is_err());
    ack.transaction_hash = signed.transaction_hash.ok_or("missing transaction hash")?;
    let acknowledged = store.acknowledge_evm_action(&principal, &key, &ack)?;
    assert!(acknowledged.acknowledged);
    host.stop();
    host.start()?;
    assert_eq!(
        acknowledged,
        store.recover_evm_action(&principal, &key, &authorization.action_key)?
    );
    conflict = authorization.clone();
    conflict.action_key = [6; 32];
    conflict.transaction.nonce = 8;
    conflict.binding_digest = [7; 32];
    assert!(store
        .authorize_evm_plan(&principal, &key, &conflict)
        .is_err());
    conflict.binding_digest = authorization.binding_digest;
    conflict.transaction.chain_id = 0;
    assert!(store
        .authorize_evm_plan(&principal, &key, &conflict)
        .is_err());
    Ok(())
}

#[test]
fn executor_certificate_cannot_authorize_or_manage_keys() -> Result<()> {
    let host = Host::new()?;
    let binding = [81; 32];
    let (handle, _) = facts(&host.call(&request(1, binding, &[], None)?)?)?;
    for operation in [0, 1, 2, 4, 7, 11] {
        let mut frame = request(
            operation,
            binding,
            if operation == 1 { &[] } else { &handle },
            None,
        )?;
        if operation >= 7 {
            frame[4..6].copy_from_slice(&3_u16.to_be_bytes());
            blob(&mut frame, b"{}")?;
        }
        let mut stream = host.connection(Some("foreign"))?;
        checked(write_frame(&mut stream, &frame, MAX))?;
        let response = checked(read_frame(&mut stream, MAX))?;
        assert_eq!(response[7], 1);
    }
    let mut frame = request(6, binding, &handle, None)?;
    frame[4..6].copy_from_slice(&3_u16.to_be_bytes());
    let mut stream = host.connection(Some("foreign"))?;
    checked(write_frame(&mut stream, &frame, MAX))?;
    let response = checked(read_frame(&mut stream, MAX))?;
    assert_eq!(response[7], 0);
    assert_eq!(response.len(), 28);
    Ok(())
}

#[test]
fn native_send_authorization_is_scoped_and_durable() -> Result<()> {
    use layerx_human_service::custody::SendPlanAuthorization;
    use layerx_wire::encode::Encoder;
    use sha2::{Digest, Sha256};
    let mut host = Host::new()?;
    let principal = PrincipalId::new("alice")?;
    let key = KeyId::new("primary")?;
    let store = Keystore::open_production(
        host.root.join("send-client"),
        77,
        host.remote("client", "beta-kms")?,
    )?;
    let public = store.create(&principal, &key, KeyClass::HumanPrimary)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let authorization = SendPlanAuthorization {
        plan_id: [1; 32],
        action_key: [2; 32],
        principal: "alice".into(),
        tenant: "tenant".into(),
        binding_digest: store.evm_binding(&principal, &key)?.digest(),
        from: [3; 32],
        to: [4; 32],
        asset: [5; 32],
        amount: 12,
        sequence: 7,
        idempotency_key: [6; 32],
        expires_at: now + 600,
        context: [7; 32],
        network: 77,
        protocol: 3,
        not_before: now,
        not_after: now + 600,
    };
    let signature = store.authorize_send(&principal, &key, &authorization)?;
    let mut message = Encoder::new(512);
    checked(message.u16(0x5301))?;
    for value in [authorization.from, authorization.to, authorization.asset] {
        checked(message.fixed(&value))?;
    }
    checked(message.u128(authorization.amount))?;
    checked(message.u64(authorization.sequence))?;
    checked(message.fixed(&authorization.idempotency_key))?;
    checked(message.u64(authorization.expires_at))?;
    checked(message.fixed(&authorization.context))?;
    checked(message.u8(0))?;
    checked(message.u8(1))?;
    checked(message.fixed(&authorization.from))?;
    checked(message.fixed(&authorization.context))?;
    checked(message.u32(authorization.network))?;
    checked(message.u16(authorization.protocol))?;
    let mut hash = Sha256::new();
    hash.update(layerx_wire::hash::Domain::SignaturePreimage.tag());
    hash.update(message.finish());
    checked(
        ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, public)
            .verify(&hash.finalize(), &signature),
    )?;
    host.stop();
    host.start()?;
    assert_eq!(
        signature,
        store.authorize_send(&principal, &key, &authorization)?
    );
    let mut changed = authorization.clone();
    changed.amount += 1;
    assert!(store.authorize_send(&principal, &key, &changed).is_err());
    changed.action_key = [8; 32];
    changed.network = 78;
    assert!(store.authorize_send(&principal, &key, &changed).is_err());
    changed.network = 77;
    changed.binding_digest = [9; 32];
    assert!(store.authorize_send(&principal, &key, &changed).is_err());
    Ok(())
}

fn transaction_signature(raw: &[u8]) -> Result<Vec<u8>> {
    let mut signature = vec![0; 65];
    let prefix = *raw.get(1).ok_or("missing RLP list")?;
    let mut offset = if prefix >= 0xf8 {
        2 + usize::from(prefix - 0xf7)
    } else {
        2
    };
    for index in 0..12 {
        let prefix = *raw.get(offset).ok_or("missing RLP field")?;
        offset += 1;
        let value = if prefix < 0x80 {
            std::slice::from_ref(&raw[offset - 1])
        } else if prefix <= 0xb7 {
            let length = usize::from(prefix - 0x80);
            let value = raw.get(offset..offset + length).ok_or("short RLP field")?;
            offset += length;
            value
        } else if prefix == 0xc0 {
            &[]
        } else {
            return Err("unsupported test RLP field".into());
        };
        if index == 9 {
            signature[64] = value.first().copied().unwrap_or(0);
        }
        if index == 10 || index == 11 {
            if value.len() > 32 {
                return Err("large signature field".into());
            }
            let end = if index == 10 { 32 } else { 64 };
            signature[end - value.len()..end].copy_from_slice(value);
        }
    }
    assert_eq!(offset, raw.len());
    Ok(signature)
}

#[test]
fn external_signature_verification_and_executor_journal_recovery() -> Result<()> {
    use layerx_human_service::custody::{
        EvmAction, EvmExternalSignature, EvmPlanAuthorization, EvmTransaction,
    };
    let mut host = Host::new()?;
    let principal = PrincipalId::new("alice")?;
    let key = KeyId::new("primary")?;
    let store = Keystore::open_production(
        host.root.join("external-client"),
        77,
        host.remote("client", "beta-kms")?,
    )?;
    store.create(&principal, &key, KeyClass::HumanPrimary)?;
    let binding = store.evm_binding(&principal, &key)?;
    let handle = store.evm_provider_reference(&principal, &key)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let authorization = EvmPlanAuthorization {
        plan_id: [1; 32],
        action_key: [2; 32],
        tenant: "tenant".into(),
        principal: "alice".into(),
        binding_digest: binding.digest(),
        wallet: store.evm_wallet(&principal, &key)?,
        not_before: now,
        not_after: now + 600,
        transaction: EvmTransaction {
            chain_id: 31337,
            nonce: 0,
            max_priority_fee_per_gas: 1,
            max_fee_per_gas: 2,
            gas_limit: 21000,
            to: [4; 20],
            value: [0; 32],
            calldata: vec![],
        },
    };
    store.authorize_evm_plan(&principal, &key, &authorization)?;
    let signed = store.sign_evm_action(&principal, &key, &authorization.action_key)?;
    let mut external = EvmExternalSignature {
        action_key: authorization.action_key,
        signature: transaction_signature(&signed.raw_transaction)?,
    };
    let executor = host.remote("foreign", "beta-kms")?;
    let accepted: EvmAction = serde_json::from_slice(&checked(executor.evm_operation(
        12,
        &binding,
        &handle,
        &serde_json::to_vec(&external)?,
    ))?)?;
    assert_eq!(accepted, signed);
    external.signature[64] += 27;
    let accepted: EvmAction = serde_json::from_slice(&checked(executor.evm_operation(
        12,
        &binding,
        &handle,
        &serde_json::to_vec(&external)?,
    ))?)?;
    assert_eq!(accepted, signed);
    host.stop();
    host.start()?;
    assert_eq!(
        signed,
        store.recover_evm_action(&principal, &key, &authorization.action_key)?
    );
    external.signature[0] ^= 1;
    assert!(executor
        .evm_operation(12, &binding, &handle, &serde_json::to_vec(&external)?)
        .is_err());
    external.signature = vec![0; 64];
    assert!(executor
        .evm_operation(12, &binding, &handle, &serde_json::to_vec(&external)?)
        .is_err());
    let foreign = k256::ecdsa::SigningKey::from_bytes((&[21; 32]).into())?;
    let (signature, recovery) = foreign.sign_prehash_recoverable(&[22; 32])?;
    external.signature = signature.to_bytes().to_vec();
    external.signature.push(recovery.to_byte());
    assert!(executor
        .evm_operation(12, &binding, &handle, &serde_json::to_vec(&external)?)
        .is_err());
    external.action_key = [23; 32];
    assert!(executor
        .evm_operation(12, &binding, &handle, &serde_json::to_vec(&external)?)
        .is_err());
    Ok(())
}

fn refuse_cross_scope(
    store: &Keystore,
    principal: &PrincipalId,
    key: &KeyId,
    authorization: &layerx_human_service::custody::EvmPlanAuthorization,
) -> Result<()> {
    use layerx_human_service::custody::{CustodyError, KmsError};
    let mut changed = authorization.clone();
    "another-tenant".clone_into(&mut changed.tenant);
    assert!(matches!(
        store.authorize_evm_plan(principal, key, &changed),
        Err(CustodyError::Kms(KmsError::Conflict))
    ));
    changed = authorization.clone();
    "bob".clone_into(&mut changed.principal);
    assert!(matches!(
        store.authorize_evm_plan(principal, key, &changed),
        Err(CustodyError::Kms(KmsError::Refused))
    ));
    let other = PrincipalId::new("bob")?;
    store.create(&other, key, KeyClass::HumanPrimary)?;
    store.evm_wallet(&other, key)?;
    assert!(store
        .recover_evm_action(&other, key, &authorization.action_key)
        .is_err());
    assert!(store
        .sign_evm_action(&other, key, &authorization.action_key)
        .is_err());
    Ok(())
}
