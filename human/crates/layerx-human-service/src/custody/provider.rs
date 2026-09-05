use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

use layerx_client::lni::transport::{
    ConnectionGate, FrameTransport, Limits, MutualTlsConfig, Tls, TransportError,
};
use layerx_crypto::disclosure::{bind, Disclosure};
use layerx_crypto::local::LocalSigner;
use layerx_crypto::signer::{sign_disclosed, Signer as _};
use layerx_types::payload::ModuleRegistry;
use sha2::{Digest as _, Sha256};
use zeroize::Zeroizing;

use super::{seal_refusal, CustodyError, EnvelopeKms, KeyClass, KeyEntropy, KmsError};

const PROVIDER_REFERENCE_LIMIT: usize = 4096;
const PROVIDER_FRAME_LIMIT: usize = 2_097_152;
const PROVIDER_MAGIC: &[u8; 4] = b"LXKP";
const PROVIDER_VERSION: u16 = 1;
const SIGNATURE_DOMAIN: &[u8] = b"LXP/v1/signature-preimage\0";
const OP_PROBE: u8 = 0;
const OP_CREATE: u8 = 1;
const OP_DESCRIBE: u8 = 2;
const OP_ROTATE: u8 = 3;
const OP_DESTROY: u8 = 4;
const OP_SIGN: u8 = 5;
const STATUS_OK: u8 = 0;
const STATUS_REFUSED: u8 = 1;
const STATUS_NOT_FOUND: u8 = 2;
const STATUS_CONFLICT: u8 = 3;
const STATUS_UNAVAILABLE: u8 = 4;
const STATUS_INTEGRITY: u8 = 5;

/// Whether a custody provider may be selected by a production service.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderDeployment {
    DevelopmentOnly,
    Production,
}

/// Redacted rotation state returned by the key-management boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RotationState {
    Stable,
    InProgress,
    Failed,
    Unknown,
}

impl RotationState {
    fn from_code(value: u8) -> Result<Self, KmsError> {
        match value {
            0 => Ok(Self::Stable),
            1 => Ok(Self::InProgress),
            2 => Ok(Self::Failed),
            3 => Ok(Self::Unknown),
            _ => Err(KmsError::InvalidResponse),
        }
    }
}

/// Opaque provider-owned handle. It is persisted but never returned by the
/// human API and its debug representation never reveals its bytes.
#[derive(Clone, Eq, PartialEq)]
pub struct ProviderKeyReference(Vec<u8>);

impl ProviderKeyReference {
    /// Creates a bounded, non-empty provider handle.
    ///
    /// # Errors
    ///
    /// Refuses empty and oversized handles before they reach storage or the
    /// remote boundary.
    pub fn new(bytes: Vec<u8>) -> Result<Self, KmsError> {
        if bytes.is_empty() || bytes.len() > PROVIDER_REFERENCE_LIMIT {
            return Err(KmsError::InvalidReference);
        }
        Ok(Self(bytes))
    }

    /// Borrows the opaque bytes for a provider implementation.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl Debug for ProviderKeyReference {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderKeyReference")
            .field("bytes", &"[redacted]")
            .field("length", &self.0.len())
            .finish()
    }
}

/// Principal-scoped identity presented on every provider operation.
#[derive(Clone, Eq, PartialEq)]
pub struct PrincipalKeyBinding {
    identity: Vec<u8>,
    digest: [u8; 32],
    network_id: u32,
    class: KeyClass,
}

impl PrincipalKeyBinding {
    pub(super) fn new(
        identity: Vec<u8>,
        network_id: u32,
        class: KeyClass,
        provider_reference: &str,
    ) -> Result<Self, CustodyError> {
        let identity_length =
            u32::try_from(identity.len()).map_err(|_| CustodyError::InvalidKeyReference)?;
        let provider_length = u32::try_from(provider_reference.len())
            .map_err(|_| CustodyError::InvalidKeyReference)?;
        let mut hasher = Sha256::new();
        hasher.update(b"layerx-human/provider-key-binding/v1\0");
        hasher.update(identity_length.to_be_bytes());
        hasher.update(&identity);
        hasher.update(provider_length.to_be_bytes());
        hasher.update(provider_reference.as_bytes());
        hasher.update(network_id.to_be_bytes());
        let digest = hasher.finalize().into();
        Ok(Self {
            identity,
            digest,
            network_id,
            class,
        })
    }

    /// Returns the non-reversible principal/key binding sent to a remote KMS.
    #[must_use]
    pub const fn digest(&self) -> [u8; 32] {
        self.digest
    }

    /// Returns the network whose signatures this key may produce.
    #[must_use]
    pub const fn network_id(&self) -> u32 {
        self.network_id
    }

    /// Returns the primary-key class held by the provider.
    #[must_use]
    pub const fn class(&self) -> KeyClass {
        self.class
    }

    pub(super) fn identity(&self) -> &[u8] {
        &self.identity
    }
}

impl Debug for PrincipalKeyBinding {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PrincipalKeyBinding")
            .field("identity", &"[redacted]")
            .field("digest", &self.digest)
            .field("network_id", &self.network_id)
            .field("class", &self.class)
            .finish()
    }
}

/// Public key facts returned by create, describe and rotate. Private material
/// is absent by construction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderKeyDescription {
    reference: ProviderKeyReference,
    public_key: [u8; 32],
    binding_digest: [u8; 32],
    rotation: RotationState,
}

impl ProviderKeyDescription {
    /// Constructs a complete provider response.
    ///
    /// # Errors
    ///
    /// Refuses zero public keys and zero binding digests.
    pub fn new(
        reference: ProviderKeyReference,
        public_key: [u8; 32],
        binding_digest: [u8; 32],
        rotation: RotationState,
    ) -> Result<Self, KmsError> {
        if public_key == [0; 32] || binding_digest == [0; 32] {
            return Err(KmsError::InvalidResponse);
        }
        Ok(Self {
            reference,
            public_key,
            binding_digest,
            rotation,
        })
    }

    #[must_use]
    pub fn reference(&self) -> &ProviderKeyReference {
        &self.reference
    }

    #[must_use]
    pub const fn public_key(&self) -> [u8; 32] {
        self.public_key
    }

    #[must_use]
    pub const fn binding_digest(&self) -> [u8; 32] {
        self.binding_digest
    }

    #[must_use]
    pub const fn rotation(&self) -> RotationState {
        self.rotation
    }
}

/// Exact request admitted to a provider signer. It always carries both the
/// canonical bytes and the structured disclosure used to approve them.
#[derive(Clone, Copy)]
pub struct ProviderSignRequest<'a> {
    canonical_bytes: &'a [u8],
    disclosure: &'a Disclosure,
    registry: &'a ModuleRegistry,
    expected_public_key: [u8; 32],
}

impl<'a> ProviderSignRequest<'a> {
    pub(super) const fn new(
        canonical_bytes: &'a [u8],
        disclosure: &'a Disclosure,
        registry: &'a ModuleRegistry,
        expected_public_key: [u8; 32],
    ) -> Self {
        Self {
            canonical_bytes,
            disclosure,
            registry,
            expected_public_key,
        }
    }

    /// Borrows the exact canonical byte string approved by the caller.
    #[must_use]
    pub const fn canonical_bytes(&self) -> &'a [u8] {
        self.canonical_bytes
    }

    /// Borrows the structured disclosure paired with the bytes.
    #[must_use]
    pub const fn disclosure(&self) -> &'a Disclosure {
        self.disclosure
    }

    /// Borrows the negotiated module registry used for re-validation.
    #[must_use]
    pub const fn registry(&self) -> &'a ModuleRegistry {
        self.registry
    }

    /// Returns the public key against which every response is verified.
    #[must_use]
    pub const fn expected_public_key(&self) -> [u8; 32] {
        self.expected_public_key
    }

    fn validate(self) -> Result<ValidatedSignRequest, CustodyError> {
        let rebound = bind(self.canonical_bytes, self.registry)
            .map_err(|error| CustodyError::Sign(layerx_crypto::signer::SignError::from(error)))?;
        if rebound != *self.disclosure {
            return Err(CustodyError::Sign(
                layerx_crypto::signer::SignError::DisclosureMismatch("canonical_bytes"),
            ));
        }
        let reencoded = self
            .disclosure
            .reencode()
            .map_err(|error| CustodyError::Sign(layerx_crypto::signer::SignError::from(error)))?;
        if !layerx_crypto::ct::eq(&reencoded, self.canonical_bytes) {
            return Err(CustodyError::Sign(
                layerx_crypto::signer::SignError::DisclosureMismatch("canonical_bytes"),
            ));
        }
        let mut hasher = Sha256::new();
        hasher.update(SIGNATURE_DOMAIN);
        hasher.update(self.canonical_bytes);
        Ok(ValidatedSignRequest {
            canonical_digest: hasher.finalize().into(),
            disclosure: encode_disclosure(self.disclosure)?,
        })
    }
}

impl Debug for ProviderSignRequest<'_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderSignRequest")
            .field("canonical_bytes", &"[redacted]")
            .field("disclosure", &"[validated at provider boundary]")
            .field("expected_public_key", &self.expected_public_key)
            .finish()
    }
}

struct ValidatedSignRequest {
    canonical_digest: [u8; 32],
    disclosure: Vec<u8>,
}

/// Future returned by object-safe KMS provider signing.
pub type KmsSignFuture<'a> =
    Pin<Box<dyn Future<Output = Result<[u8; 64], CustodyError>> + Send + 'a>>;

/// Public production custody boundary. Implementations own key creation,
/// description, rotation, destruction and signing; none can return private key
/// material through this interface.
pub trait KmsProvider: Debug + Send + Sync {
    /// Returns the bounded, non-secret provider or partition reference.
    #[must_use]
    fn provider_reference(&self) -> &str;

    /// Declares whether this implementation is permitted in production.
    #[must_use]
    fn deployment(&self) -> ProviderDeployment;

    /// Probes the authenticated provider boundary.
    ///
    /// # Errors
    ///
    /// Returns the exact availability, authentication or provider refusal.
    fn probe(&self) -> Result<(), KmsError>;

    /// Creates a primary key inside the provider.
    ///
    /// # Errors
    ///
    /// Returns a typed provider refusal and never partial key material.
    fn create_key(&self, binding: &PrincipalKeyBinding)
        -> Result<ProviderKeyDescription, KmsError>;

    /// Development-only deterministic creation used by local fixtures.
    ///
    /// # Errors
    ///
    /// Returns `DevelopmentOnly` unless the implementation is explicitly a
    /// development provider, or its typed creation refusal.
    fn create_development_key(
        &self,
        _binding: &PrincipalKeyBinding,
        _entropy: KeyEntropy,
    ) -> Result<ProviderKeyDescription, KmsError> {
        Err(KmsError::DevelopmentOnly)
    }

    /// Describes an existing provider key without exporting it.
    ///
    /// # Errors
    ///
    /// Returns typed absence, availability, authentication and integrity
    /// refusals.
    fn describe_key(
        &self,
        binding: &PrincipalKeyBinding,
        reference: &ProviderKeyReference,
    ) -> Result<ProviderKeyDescription, KmsError>;

    /// Rotates an existing provider key and returns its new public facts.
    ///
    /// # Errors
    ///
    /// Returns a typed lifecycle or provider refusal.
    fn rotate_key(
        &self,
        binding: &PrincipalKeyBinding,
        reference: &ProviderKeyReference,
    ) -> Result<ProviderKeyDescription, KmsError>;

    /// Rotates a stable handle only from the expected current public key.
    ///
    /// # Errors
    /// Refuses providers without compare-and-swap rotation support.
    fn rotate_key_if_current(
        &self,
        _binding: &PrincipalKeyBinding,
        _reference: &ProviderKeyReference,
        _expected_public_key: [u8; 32],
    ) -> Result<ProviderKeyDescription, KmsError> {
        Err(KmsError::Refused)
    }

    /// Destroys a provider key. No key bytes are returned.
    ///
    /// # Errors
    ///
    /// Returns a typed lifecycle or provider refusal.
    fn destroy_key(
        &self,
        binding: &PrincipalKeyBinding,
        reference: &ProviderKeyReference,
    ) -> Result<(), KmsError>;

    /// Signs only a complete disclosure-bound request and returns only the
    /// public signature bytes.
    ///
    /// # Errors
    ///
    /// The returned future resolves to a disclosure, provider, integrity or
    /// signature refusal.
    fn sign<'a>(
        &'a self,
        binding: &'a PrincipalKeyBinding,
        reference: &'a ProviderKeyReference,
        request: ProviderSignRequest<'a>,
    ) -> KmsSignFuture<'a>;
}

/// Signer handle owned by the service but backed exclusively by a provider
/// key reference. It contains no private material and has no export method.
#[derive(Clone)]
pub struct RemoteCustodySigner {
    provider: Arc<dyn KmsProvider>,
    binding: PrincipalKeyBinding,
    reference: ProviderKeyReference,
    public_key: [u8; 32],
    class: KeyClass,
}

impl RemoteCustodySigner {
    pub(super) fn new(
        provider: Arc<dyn KmsProvider>,
        binding: PrincipalKeyBinding,
        description: ProviderKeyDescription,
        class: KeyClass,
    ) -> Self {
        Self {
            provider,
            binding,
            reference: description.reference,
            public_key: description.public_key,
            class,
        }
    }

    /// Returns the public verification key for this provider handle.
    #[must_use]
    pub const fn public_key(&self) -> [u8; 32] {
        self.public_key
    }

    /// Returns the primary-key class without exposing the provider handle.
    #[must_use]
    pub const fn class(&self) -> KeyClass {
        self.class
    }

    /// Sends the exact canonical bytes and matching disclosure to the provider
    /// and returns only the signature.
    ///
    /// # Errors
    ///
    /// Refuses a changed disclosure, a provider failure or a signature that
    /// does not verify against the recorded public key.
    pub async fn sign_disclosed(
        &self,
        canonical_bytes: &[u8],
        disclosure: &Disclosure,
        registry: &ModuleRegistry,
    ) -> Result<[u8; 64], CustodyError> {
        self.provider
            .sign(
                &self.binding,
                &self.reference,
                ProviderSignRequest::new(canonical_bytes, disclosure, registry, self.public_key),
            )
            .await
    }
}

impl Debug for RemoteCustodySigner {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RemoteCustodySigner")
            .field("provider", &"[remote KMS/HSM]")
            .field("binding", &self.binding)
            .field("reference", &"[redacted]")
            .field("public_key", &self.public_key)
            .field("class", &self.class)
            .finish()
    }
}

/// Production provider speaking the bounded LayerX KMS protocol over mutual
/// TLS to a remote KMS/HSM gateway. Every operation opens a fresh authenticated
/// connection, so a failed connection cannot silently reuse cached authority.
pub struct RemoteKmsProvider {
    provider_reference: String,
    endpoint: SocketAddr,
    server_name: String,
    tls: MutualTlsConfig,
    gate: ConnectionGate,
    limits: Limits,
}

impl RemoteKmsProvider {
    /// Configures a production remote provider from an authenticated transport.
    ///
    /// # Errors
    ///
    /// Refuses invalid provider references, empty server identities and
    /// transport limits before the provider can be selected. Production
    /// startup authenticates the parsed DNS identity through `probe`.
    pub fn new(
        provider_reference: impl Into<String>,
        endpoint: SocketAddr,
        server_name: impl Into<String>,
        tls: MutualTlsConfig,
        limits: Limits,
    ) -> Result<Self, CustodyError> {
        let provider_reference = provider_reference.into();
        if provider_reference.is_empty()
            || provider_reference.len() > super::KEY_REFERENCE_LIMIT
            || provider_reference.as_bytes().contains(&0)
        {
            return Err(CustodyError::InvalidKeyReference);
        }
        let server_name = server_name.into();
        if server_name.is_empty() || server_name.as_bytes().contains(&0) {
            return Err(CustodyError::Kms(KmsError::Authentication));
        }
        let limits = limits
            .validate()
            .map_err(|_| CustodyError::Kms(KmsError::InvalidConfiguration))?;
        if limits.maximum_frame_bytes > PROVIDER_FRAME_LIMIT {
            return Err(CustodyError::Kms(KmsError::InvalidConfiguration));
        }
        Ok(Self {
            provider_reference,
            endpoint,
            server_name,
            tls,
            gate: ConnectionGate::new(limits.maximum_connections),
            limits,
        })
    }

    fn call(&self, operation: u8, request: Vec<u8>) -> Result<Vec<u8>, KmsError> {
        self.call_version(operation, PROVIDER_VERSION, request)
    }

    fn call_version(
        &self,
        operation: u8,
        version: u16,
        request: Vec<u8>,
    ) -> Result<Vec<u8>, KmsError> {
        if request.len() > self.limits.maximum_frame_bytes {
            return Err(KmsError::InvalidConfiguration);
        }
        let server_name = self
            .server_name
            .clone()
            .try_into()
            .map_err(|_| KmsError::Authentication)?;
        let mut transport = Tls::connect(
            self.endpoint,
            server_name,
            &self.tls,
            &self.gate,
            self.limits,
        )
        .map_err(map_transport)?;
        transport.send(&request).map_err(map_transport)?;
        let response = transport.receive().map_err(map_transport)?;
        decode_response(operation, version, &response)
    }

    fn key_operation(
        &self,
        operation: u8,
        binding: &PrincipalKeyBinding,
        reference: Option<&ProviderKeyReference>,
    ) -> Result<ProviderKeyDescription, KmsError> {
        let request = encode_key_request(operation, &self.provider_reference, binding, reference)?;
        let response = self.call(operation, request)?;
        decode_description(&response, binding)
    }
}

impl Debug for RemoteKmsProvider {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RemoteKmsProvider")
            .field("provider_reference", &self.provider_reference)
            .field("endpoint", &"[mutually authenticated remote endpoint]")
            .field("server_name", &self.server_name)
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl KmsProvider for RemoteKmsProvider {
    fn provider_reference(&self) -> &str {
        &self.provider_reference
    }

    fn deployment(&self) -> ProviderDeployment {
        ProviderDeployment::Production
    }

    fn probe(&self) -> Result<(), KmsError> {
        let request = encode_header(OP_PROBE, &self.provider_reference)?;
        let response = self.call(OP_PROBE, request)?;
        if response.is_empty() {
            Ok(())
        } else {
            Err(KmsError::InvalidResponse)
        }
    }

    fn create_key(
        &self,
        binding: &PrincipalKeyBinding,
    ) -> Result<ProviderKeyDescription, KmsError> {
        self.key_operation(OP_CREATE, binding, None)
    }

    fn describe_key(
        &self,
        binding: &PrincipalKeyBinding,
        reference: &ProviderKeyReference,
    ) -> Result<ProviderKeyDescription, KmsError> {
        self.key_operation(OP_DESCRIBE, binding, Some(reference))
    }

    fn rotate_key(
        &self,
        binding: &PrincipalKeyBinding,
        reference: &ProviderKeyReference,
    ) -> Result<ProviderKeyDescription, KmsError> {
        self.key_operation(OP_ROTATE, binding, Some(reference))
    }

    fn rotate_key_if_current(
        &self,
        binding: &PrincipalKeyBinding,
        reference: &ProviderKeyReference,
        expected_public_key: [u8; 32],
    ) -> Result<ProviderKeyDescription, KmsError> {
        let mut request = encode_key_request(
            OP_ROTATE,
            &self.provider_reference,
            binding,
            Some(reference),
        )?;
        request[4..6].copy_from_slice(&2_u16.to_be_bytes());
        request.extend_from_slice(&expected_public_key);
        let response = self.call_version(OP_ROTATE, 2, request)?;
        decode_description(&response, binding)
    }

    fn destroy_key(
        &self,
        binding: &PrincipalKeyBinding,
        reference: &ProviderKeyReference,
    ) -> Result<(), KmsError> {
        let request = encode_key_request(
            OP_DESTROY,
            &self.provider_reference,
            binding,
            Some(reference),
        )?;
        let response = self.call(OP_DESTROY, request)?;
        if response.is_empty() {
            Ok(())
        } else {
            Err(KmsError::InvalidResponse)
        }
    }

    fn sign<'a>(
        &'a self,
        binding: &'a PrincipalKeyBinding,
        reference: &'a ProviderKeyReference,
        request: ProviderSignRequest<'a>,
    ) -> KmsSignFuture<'a> {
        Box::pin(async move {
            let validated = request.validate()?;
            let frame = encode_sign_request(
                &self.provider_reference,
                binding,
                reference,
                request.canonical_bytes,
                &validated,
            )?;
            let response = self.call(OP_SIGN, frame)?;
            let signature: [u8; 64] = response
                .as_slice()
                .try_into()
                .map_err(|_| CustodyError::Kms(KmsError::InvalidResponse))?;
            layerx_crypto::ed25519::verify_digest(
                &request.expected_public_key,
                &signature,
                &validated.canonical_digest,
            )
            .map_err(|_| {
                CustodyError::Sign(layerx_crypto::signer::SignError::ReturnedSignatureInvalid)
            })?;
            Ok(signature)
        })
    }
}

impl KmsProvider for EnvelopeKms {
    fn provider_reference(&self) -> &str {
        &self.key_reference
    }

    fn deployment(&self) -> ProviderDeployment {
        ProviderDeployment::DevelopmentOnly
    }

    fn probe(&self) -> Result<(), KmsError> {
        self.root_secret().map(|_| ())
    }

    fn create_key(
        &self,
        binding: &PrincipalKeyBinding,
    ) -> Result<ProviderKeyDescription, KmsError> {
        let mut seed = Zeroizing::new([0_u8; 32]);
        let mut salt = [0_u8; 16];
        let mut nonce = [0_u8; 24];
        getrandom::fill(&mut *seed).map_err(|_| KmsError::Unavailable)?;
        getrandom::fill(&mut salt).map_err(|_| KmsError::Unavailable)?;
        getrandom::fill(&mut nonce).map_err(|_| KmsError::Unavailable)?;
        let entropy = KeyEntropy::new(*seed, salt, nonce).map_err(|_| KmsError::Refused)?;
        self.create_development_key(binding, entropy)
    }

    fn create_development_key(
        &self,
        binding: &PrincipalKeyBinding,
        entropy: KeyEntropy,
    ) -> Result<ProviderKeyDescription, KmsError> {
        let KeyEntropy { seed, envelope } = entropy;
        let public_key = LocalSigner::new(*seed).public_key();
        let root = self.root_secret()?;
        let envelope = layerx_crypto::keystore::Keystore::seal(
            &seed,
            &root,
            binding.identity(),
            binding.network_id,
            envelope,
        )
        .map_err(seal_refusal)?;
        let reference = ProviderKeyReference::new(envelope.to_bytes().map_err(seal_refusal)?)?;
        ProviderKeyDescription::new(reference, public_key, binding.digest, RotationState::Stable)
    }

    fn describe_key(
        &self,
        binding: &PrincipalKeyBinding,
        reference: &ProviderKeyReference,
    ) -> Result<ProviderKeyDescription, KmsError> {
        let root = self.root_secret()?;
        let envelope = layerx_crypto::keystore::Keystore::from_bytes(reference.as_bytes())
            .map_err(seal_refusal)?;
        let seed = envelope
            .open(&root, binding.identity(), binding.network_id)
            .map_err(seal_refusal)?;
        let public_key = LocalSigner::from_secret(seed).public_key();
        ProviderKeyDescription::new(
            reference.clone(),
            public_key,
            binding.digest,
            RotationState::Stable,
        )
    }

    fn rotate_key(
        &self,
        binding: &PrincipalKeyBinding,
        _reference: &ProviderKeyReference,
    ) -> Result<ProviderKeyDescription, KmsError> {
        self.create_key(binding)
    }

    fn destroy_key(
        &self,
        _binding: &PrincipalKeyBinding,
        _reference: &ProviderKeyReference,
    ) -> Result<(), KmsError> {
        Ok(())
    }

    fn sign<'a>(
        &'a self,
        binding: &'a PrincipalKeyBinding,
        reference: &'a ProviderKeyReference,
        request: ProviderSignRequest<'a>,
    ) -> KmsSignFuture<'a> {
        Box::pin(async move {
            let _ = request.validate()?;
            let root = self.root_secret()?;
            let envelope = layerx_crypto::keystore::Keystore::from_bytes(reference.as_bytes())
                .map_err(seal_refusal)?;
            let seed = envelope
                .open(&root, binding.identity(), binding.network_id)
                .map_err(seal_refusal)?;
            let signer = LocalSigner::from_secret(seed);
            if signer.public_key() != request.expected_public_key {
                return Err(CustodyError::Kms(KmsError::Integrity));
            }
            sign_disclosed(
                &signer,
                request.canonical_bytes,
                request.disclosure,
                request.registry,
            )
            .await
            .map(|signature| *signature.as_bytes())
            .map_err(CustodyError::Sign)
        })
    }
}

fn encode_header(operation: u8, provider_reference: &str) -> Result<Vec<u8>, KmsError> {
    let mut writer = WireWriter::new();
    writer.fixed(PROVIDER_MAGIC)?;
    writer.u16(PROVIDER_VERSION)?;
    writer.u8(operation)?;
    writer.bytes(provider_reference.as_bytes(), super::KEY_REFERENCE_LIMIT)?;
    Ok(writer.finish())
}

fn encode_key_request(
    operation: u8,
    provider_reference: &str,
    binding: &PrincipalKeyBinding,
    reference: Option<&ProviderKeyReference>,
) -> Result<Vec<u8>, KmsError> {
    let mut writer = WireWriter::from_bytes(encode_header(operation, provider_reference)?);
    writer.fixed(&binding.digest)?;
    writer.u32(binding.network_id)?;
    writer.u8(binding.class.code())?;
    match reference {
        Some(reference) => writer.bytes(reference.as_bytes(), PROVIDER_REFERENCE_LIMIT)?,
        None => writer.bytes(&[], PROVIDER_REFERENCE_LIMIT)?,
    }
    Ok(writer.finish())
}

fn encode_sign_request(
    provider_reference: &str,
    binding: &PrincipalKeyBinding,
    reference: &ProviderKeyReference,
    canonical: &[u8],
    validated: &ValidatedSignRequest,
) -> Result<Vec<u8>, CustodyError> {
    let mut writer = WireWriter::from_bytes(encode_header(OP_SIGN, provider_reference)?);
    writer.fixed(&binding.digest)?;
    writer.u32(binding.network_id)?;
    writer.u8(binding.class.code())?;
    writer.bytes(reference.as_bytes(), PROVIDER_REFERENCE_LIMIT)?;
    writer.fixed(&validated.canonical_digest)?;
    writer.bytes(canonical, PROVIDER_FRAME_LIMIT)?;
    writer.bytes(&validated.disclosure, PROVIDER_FRAME_LIMIT)?;
    let frame = writer.finish();
    if frame.len() > PROVIDER_FRAME_LIMIT {
        return Err(CustodyError::Kms(KmsError::InvalidConfiguration));
    }
    Ok(frame)
}

fn encode_disclosure(disclosure: &Disclosure) -> Result<Vec<u8>, CustodyError> {
    let mut writer = WireWriter::new();
    writer.u8(1)?;
    writer.u32(disclosure.activity_type.value())?;
    writer.bytes(&disclosure.actor, 255)?;
    writer.bytes(&disclosure.authority, 524_288)?;
    writer.u32(
        u32::try_from(disclosure.counterparties.len())
            .map_err(|_| CustodyError::Kms(KmsError::InvalidConfiguration))?,
    )?;
    for counterparty in &disclosure.counterparties {
        writer.u8(match counterparty.role {
            layerx_crypto::disclosure::CounterpartyRole::Payer => 1,
            layerx_crypto::disclosure::CounterpartyRole::Recipient => 2,
        })?;
        writer.fixed(&counterparty.account)?;
    }
    writer.u32(
        u32::try_from(disclosure.amounts.len())
            .map_err(|_| CustodyError::Kms(KmsError::InvalidConfiguration))?,
    )?;
    for amount in &disclosure.amounts {
        writer.u8(match amount.role {
            layerx_crypto::disclosure::AmountRole::Transfer => 1,
            layerx_crypto::disclosure::AmountRole::SpendingLimit => 2,
        })?;
        writer.fixed(&amount.value.to_be_bytes())?;
    }
    writer.fixed(&disclosure.asset)?;
    writer.fixed(&disclosure.fee_limit.to_be_bytes())?;
    writer.fixed(&disclosure.expiry.not_before.to_be_bytes())?;
    writer.fixed(&disclosure.expiry.not_after.to_be_bytes())?;
    writer.fixed(&disclosure.expiry.payload_expires_at.to_be_bytes())?;
    writer.fixed(&disclosure.idempotency_key)?;
    match disclosure.evm_payout_binding {
        Some(binding) => {
            writer.u8(1)?;
            writer.fixed(&binding.did_id)?;
            writer.u32(binding.network_id)?;
            writer.fixed(&binding.payout_address)?;
            writer.fixed(&binding.ownership_signature_digest)?;
        }
        None => writer.u8(0)?,
    }
    Ok(writer.finish())
}

fn decode_response(operation: u8, version: u16, bytes: &[u8]) -> Result<Vec<u8>, KmsError> {
    let mut reader = WireReader::new(bytes);
    if reader.fixed(4)? != PROVIDER_MAGIC || reader.u16()? != version || reader.u8()? != operation {
        return Err(KmsError::InvalidResponse);
    }
    let status = reader.u8()?;
    if status == STATUS_OK {
        return Ok(reader.remaining().to_vec());
    }
    if !reader.remaining().is_empty() {
        return Err(KmsError::InvalidResponse);
    }
    match status {
        STATUS_REFUSED => Err(KmsError::Refused),
        STATUS_NOT_FOUND => Err(KmsError::KeyNotFound),
        STATUS_CONFLICT => Err(KmsError::Conflict),
        STATUS_UNAVAILABLE => Err(KmsError::Unavailable),
        STATUS_INTEGRITY => Err(KmsError::Integrity),
        _ => Err(KmsError::InvalidResponse),
    }
}

fn decode_description(
    bytes: &[u8],
    binding: &PrincipalKeyBinding,
) -> Result<ProviderKeyDescription, KmsError> {
    let mut reader = WireReader::new(bytes);
    let reference = ProviderKeyReference::new(reader.bytes(PROVIDER_REFERENCE_LIMIT)?.to_vec())?;
    let public_key = reader
        .fixed(32)?
        .try_into()
        .map_err(|_| KmsError::InvalidResponse)?;
    let binding_digest = reader
        .fixed(32)?
        .try_into()
        .map_err(|_| KmsError::InvalidResponse)?;
    let rotation = RotationState::from_code(reader.u8()?)?;
    reader.finish()?;
    if !layerx_crypto::ct::eq_fixed(&binding_digest, &binding.digest) {
        return Err(KmsError::Integrity);
    }
    ProviderKeyDescription::new(reference, public_key, binding_digest, rotation)
}

fn map_transport(error: TransportError) -> KmsError {
    match error {
        TransportError::Deadline => KmsError::Timeout,
        TransportError::TlsConfiguration => KmsError::Authentication,
        TransportError::Frame(_) | TransportError::PeerShutdown | TransportError::Backpressure => {
            KmsError::InvalidResponse
        }
        TransportError::ConnectionFailure(_)
        | TransportError::ConnectionLimit
        | TransportError::StreamLimit => KmsError::Unavailable,
    }
}

struct WireWriter {
    bytes: Vec<u8>,
}

impl WireWriter {
    fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    fn from_bytes(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    fn u8(&mut self, value: u8) -> Result<(), KmsError> {
        self.extend(&[value])
    }

    fn u16(&mut self, value: u16) -> Result<(), KmsError> {
        self.extend(&value.to_be_bytes())
    }

    fn u32(&mut self, value: u32) -> Result<(), KmsError> {
        self.extend(&value.to_be_bytes())
    }

    fn fixed(&mut self, value: &[u8]) -> Result<(), KmsError> {
        self.extend(value)
    }

    fn bytes(&mut self, value: &[u8], maximum: usize) -> Result<(), KmsError> {
        if value.len() > maximum {
            return Err(KmsError::InvalidConfiguration);
        }
        let length = u32::try_from(value.len()).map_err(|_| KmsError::InvalidConfiguration)?;
        self.u32(length)?;
        self.extend(value)
    }

    fn extend(&mut self, value: &[u8]) -> Result<(), KmsError> {
        let next = self
            .bytes
            .len()
            .checked_add(value.len())
            .ok_or(KmsError::InvalidConfiguration)?;
        if next > PROVIDER_FRAME_LIMIT {
            return Err(KmsError::InvalidConfiguration);
        }
        self.bytes.extend_from_slice(value);
        Ok(())
    }

    fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

struct WireReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> WireReader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn fixed(&mut self, length: usize) -> Result<&'a [u8], KmsError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(KmsError::InvalidResponse)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(KmsError::InvalidResponse)?;
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, KmsError> {
        Ok(self.fixed(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, KmsError> {
        let bytes = self
            .fixed(2)?
            .try_into()
            .map_err(|_| KmsError::InvalidResponse)?;
        Ok(u16::from_be_bytes(bytes))
    }

    fn u32(&mut self) -> Result<u32, KmsError> {
        let bytes = self
            .fixed(4)?
            .try_into()
            .map_err(|_| KmsError::InvalidResponse)?;
        Ok(u32::from_be_bytes(bytes))
    }

    fn bytes(&mut self, maximum: usize) -> Result<&'a [u8], KmsError> {
        let length = usize::try_from(self.u32()?).map_err(|_| KmsError::InvalidResponse)?;
        if length > maximum {
            return Err(KmsError::InvalidResponse);
        }
        self.fixed(length)
    }

    fn remaining(&self) -> &'a [u8] {
        &self.bytes[self.offset..]
    }

    fn finish(self) -> Result<(), KmsError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(KmsError::InvalidResponse)
        }
    }
}
