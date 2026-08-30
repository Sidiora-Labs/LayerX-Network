//! KMS-backed custody for human and managed-agent signing keys.

mod provider;
mod sessions;
mod signer;

pub use provider::{
    KmsProvider, PrincipalKeyBinding, ProviderDeployment, ProviderKeyDescription,
    ProviderKeyReference, ProviderSignRequest, RemoteCustodySigner, RemoteKmsProvider,
    RotationState,
};
pub use sessions::{
    AgentContractError, AgentSessionContract, AgentSessionProvision, AgentSessionSecret,
    ManagedAgentState, PlainTime, ProtocolIdentitySnapshot, ProvisionEvidence, RenewalOutcome,
    RevocationEvidence, RevocationOutcome, RevocationReason, RotationEvidence, RotationJourney,
    RotationJourneyState, RotationObservation, RotationSubject, RotationSubmission,
    SessionEntropySource, SessionKeyEntropy, SessionKeyError, SessionKeyProvisioner, SessionLease,
    SessionLeaseState, SessionPolicy, SessionTarget, SuspensionEvidence,
};

pub use signer::{
    CustodySigner, Operation, SignAuthorization, SignRequest, SignatureGrant, SigningLimits,
    StepUpEvidence,
};

use std::fmt::{Display, Formatter};
use std::fs;
use std::io;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use layerx_crypto::keystore::{KeystoreEntropy, KeystoreError};
use layerx_crypto::signer::SignError;
use zeroize::Zeroizing;

use crate::audit::AuditError;
use crate::store::{PrincipalId, StoreError};

const IDENTITY_DOMAIN: &[u8] = b"layerx-human-custody/v1";
const RECORD_MAGIC: &[u8; 4] = b"LXCK";
const RECORD_VERSION: u8 = 2;
const LEGACY_RECORD_VERSION: u8 = 1;
const PRINCIPALS_DIR: &str = "principals";
const KEY_FILE_SUFFIX: &str = ".key";
const TEMP_FILE_SUFFIX: &str = ".key.tmp";
const IDENTIFIER_LIMIT: usize = 128;
const KEY_REFERENCE_LIMIT: usize = 256;
const KMS_SECRET_MINIMUM: usize = 32;
const KMS_SECRET_MAXIMUM: usize = 4096;

pub(crate) fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= IDENTIFIER_LIMIT
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        })
}

/// A validated custody key identifier, the only handle that reaches sealed
/// key records and the sole source of one on-disk file name.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct KeyId(String);

impl KeyId {
    /// Creates a bounded identifier limited to `a-z`, `0-9`, `-` and `_` so no
    /// identifier can traverse or alias another principal's key subtree.
    ///
    /// # Errors
    ///
    /// Rejects empty, oversize and out-of-charset identifiers.
    pub fn new(value: impl Into<String>) -> Result<Self, CustodyError> {
        let value = value.into();
        if valid_identifier(&value) {
            Ok(Self(value))
        } else {
            Err(CustodyError::InvalidKeyId)
        }
    }

    /// Returns the identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The principal class a custody key signs for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyClass {
    HumanPrimary,
    AgentPrimary,
}

impl KeyClass {
    const fn code(self) -> u8 {
        match self {
            Self::HumanPrimary => 1,
            Self::AgentPrimary => 2,
        }
    }

    fn from_code(value: u8) -> Result<Self, CustodyError> {
        match value {
            1 => Ok(Self::HumanPrimary),
            2 => Ok(Self::AgentPrimary),
            _ => Err(CustodyError::CorruptRecord("unknown key class")),
        }
    }

    /// Returns the stable audit label for this class.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::HumanPrimary => "human-primary",
            Self::AgentPrimary => "agent-primary",
        }
    }
}

/// Development-only caller-supplied entropy for the file-envelope provider.
/// Production providers generate primary keys remotely and never accept this
/// type. Its private seed and envelope entropy are zeroized on release.
pub struct KeyEntropy {
    seed: Zeroizing<[u8; 32]>,
    envelope: KeystoreEntropy,
}

impl KeyEntropy {
    /// Accepts independently generated seed, salt and nonce bytes.
    ///
    /// # Errors
    ///
    /// Refuses any all-zero value, which is never valid generated entropy.
    pub fn new(seed: [u8; 32], salt: [u8; 16], nonce: [u8; 24]) -> Result<Self, CustodyError> {
        let envelope =
            KeystoreEntropy::new(salt, nonce).map_err(|_| CustodyError::InvalidEntropy)?;
        let seed = Zeroizing::new(seed);
        if layerx_crypto::ct::eq_fixed(&seed, &[0_u8; 32]) {
            return Err(CustodyError::InvalidEntropy);
        }
        Ok(Self { seed, envelope })
    }
}

impl std::fmt::Debug for KeyEntropy {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("KeyEntropy([redacted])")
    }
}

/// Typed refusal taxonomy of the key-management boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KmsError {
    /// The KMS root material cannot be reached.
    Unavailable,
    /// The KMS was reached and refused the operation.
    Refused,
    /// The sealed envelope bytes are malformed.
    InvalidEnvelope,
    /// Provider configuration cannot satisfy the bounded protocol.
    InvalidConfiguration,
    /// A provider key reference is empty or exceeds its bound.
    InvalidReference,
    /// The mutually authenticated provider identity could not be established.
    Authentication,
    /// The provider exceeded the declared operation deadline.
    Timeout,
    /// The provider returned a malformed or cross-operation response.
    InvalidResponse,
    /// The named provider key does not exist.
    KeyNotFound,
    /// The provider rejected a conflicting lifecycle operation.
    Conflict,
    /// The provider response did not match the principal-scoped key record.
    Integrity,
    /// A development-only operation was requested from a production provider.
    DevelopmentOnly,
}

impl Display for KmsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Unavailable => "key-management service is unavailable",
            Self::Refused => "key-management service refused the operation",
            Self::InvalidEnvelope => "sealed key envelope is malformed",
            Self::InvalidConfiguration => "key-management service configuration is invalid",
            Self::InvalidReference => "key-management key reference is invalid",
            Self::Authentication => "key-management service authentication failed",
            Self::Timeout => "key-management service operation timed out",
            Self::InvalidResponse => "key-management service returned an invalid response",
            Self::KeyNotFound => "key-management service key does not exist",
            Self::Conflict => "key-management service lifecycle operation conflicted",
            Self::Integrity => "key-management key reference integrity failed",
            Self::DevelopmentOnly => "operation is available only with the development provider",
        })
    }
}

impl std::error::Error for KmsError {}

/// Development-only envelope provider. It performs authenticated envelope
/// encryption under a file-mounted root secret and is rejected by
/// [`Keystore::open_production`]. Production uses [`RemoteKmsProvider`].
pub struct EnvelopeKms {
    key_reference: String,
    secret_path: PathBuf,
}

impl EnvelopeKms {
    /// Configures the provider with its root key reference and secret mount.
    ///
    /// # Errors
    ///
    /// Rejects empty, oversize or NUL-bearing key references.
    pub fn new(
        key_reference: impl Into<String>,
        secret_path: impl Into<PathBuf>,
    ) -> Result<Self, CustodyError> {
        let key_reference = key_reference.into();
        if key_reference.is_empty()
            || key_reference.len() > KEY_REFERENCE_LIMIT
            || key_reference.as_bytes().contains(&0)
        {
            return Err(CustodyError::InvalidKeyReference);
        }
        Ok(Self {
            key_reference,
            secret_path: secret_path.into(),
        })
    }

    pub(super) fn root_secret(&self) -> Result<Zeroizing<Vec<u8>>, KmsError> {
        let file = fs::File::open(&self.secret_path).map_err(|_| KmsError::Unavailable)?;
        let mut secret = Zeroizing::new(Vec::with_capacity(KMS_SECRET_MINIMUM));
        file.take(
            u64::try_from(KMS_SECRET_MAXIMUM.saturating_add(1)).map_err(|_| KmsError::Refused)?,
        )
        .read_to_end(&mut secret)
        .map_err(|_| KmsError::Unavailable)?;
        if !(KMS_SECRET_MINIMUM..=KMS_SECRET_MAXIMUM).contains(&secret.len()) {
            return Err(KmsError::Refused);
        }
        Ok(secret)
    }
}

impl std::fmt::Debug for EnvelopeKms {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EnvelopeKms")
            .field("key_reference", &self.key_reference)
            .finish_non_exhaustive()
    }
}

pub(super) fn seal_refusal(error: KeystoreError) -> KmsError {
    match error {
        KeystoreError::MalformedStorage => KmsError::InvalidEnvelope,
        KeystoreError::InvalidInput
        | KeystoreError::IdentityMismatch
        | KeystoreError::NetworkMismatch
        | KeystoreError::KeyDerivation
        | KeystoreError::AuthenticationFailed => KmsError::Refused,
    }
}

/// Errors from the custody keystore and signer.
#[derive(Debug)]
pub enum CustodyError {
    InvalidKeyId,
    InvalidKeyReference,
    InvalidNetwork,
    InvalidEntropy,
    InvalidLimits,
    InvalidEvidence,
    DevelopmentProviderInProduction,
    CorruptState(&'static str),
    KeyExists,
    KeyNotFound,
    CorruptRecord(&'static str),
    Io(io::Error),
    Kms(KmsError),
    Sign(SignError),
    StepUpRequired,
    StepUpOperationMismatch,
    StepUpMismatch,
    StepUpNotYetValid,
    StepUpExpired,
    StepUpReplayed,
    NonMonotonicTime,
    ThroughputExceeded { retry_at: u64 },
    CoordinationUnavailable,
    Audit(AuditError),
    Store(StoreError),
}

impl CustodyError {
    /// Returns the stable machine code this refusal is audited under.
    #[must_use]
    pub const fn refusal_code(&self) -> &'static str {
        match self {
            Self::InvalidKeyId => "invalid-key-id",
            Self::InvalidKeyReference => "invalid-key-reference",
            Self::InvalidNetwork => "invalid-network",
            Self::InvalidEntropy => "invalid-entropy",
            Self::InvalidLimits => "invalid-limits",
            Self::InvalidEvidence => "invalid-evidence",
            Self::DevelopmentProviderInProduction => "development-kms-in-production",
            Self::CorruptState(_) => "corrupt-state",
            Self::KeyExists => "key-exists",
            Self::KeyNotFound => "key-not-found",
            Self::CorruptRecord(_) => "corrupt-record",
            Self::Io(_) => "storage-io",
            Self::Kms(KmsError::Unavailable) => "kms-unavailable",
            Self::Kms(KmsError::Refused) => "kms-refused",
            Self::Kms(KmsError::InvalidEnvelope) => "kms-invalid-envelope",
            Self::Kms(KmsError::InvalidConfiguration) => "kms-invalid-configuration",
            Self::Kms(KmsError::InvalidReference) => "kms-invalid-reference",
            Self::Kms(KmsError::Authentication) => "kms-authentication",
            Self::Kms(KmsError::Timeout) => "kms-timeout",
            Self::Kms(KmsError::InvalidResponse) => "kms-invalid-response",
            Self::Kms(KmsError::KeyNotFound) => "kms-key-not-found",
            Self::Kms(KmsError::Conflict) => "kms-conflict",
            Self::Kms(KmsError::Integrity) => "kms-integrity",
            Self::Kms(KmsError::DevelopmentOnly) => "kms-development-only",
            Self::Sign(SignError::DisclosureMismatch(_)) => "disclosure-mismatch",
            Self::Sign(SignError::InvalidDisclosure) => "disclosure-invalid",
            Self::Sign(_) => "signer-refused",
            Self::StepUpRequired => "step-up-required",
            Self::StepUpOperationMismatch => "step-up-operation-mismatch",
            Self::StepUpMismatch => "step-up-mismatch",
            Self::StepUpNotYetValid => "step-up-not-yet-valid",
            Self::StepUpExpired => "step-up-expired",
            Self::StepUpReplayed => "step-up-replayed",
            Self::NonMonotonicTime => "non-monotonic-time",
            Self::ThroughputExceeded { .. } => "throughput-exceeded",
            Self::CoordinationUnavailable => "coordination-unavailable",
            Self::Audit(_) => "audit-append-failed",
            Self::Store(_) => "store-refused",
        }
    }
}

impl Display for CustodyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidKeyId => formatter.write_str("invalid custody key identifier"),
            Self::InvalidKeyReference => formatter.write_str("invalid KMS key reference"),
            Self::InvalidNetwork => formatter.write_str("network zero is never a custody scope"),
            Self::InvalidEntropy => formatter.write_str("key entropy is invalid"),
            Self::InvalidLimits => formatter.write_str("signing limits must be non-zero"),
            Self::InvalidEvidence => formatter.write_str("step-up evidence is invalid"),
            Self::DevelopmentProviderInProduction => {
                formatter.write_str("development file-envelope KMS is forbidden in production")
            }
            Self::CorruptState(reason) => write!(formatter, "corrupt custody state: {reason}"),
            Self::KeyExists => formatter.write_str("custody key already exists"),
            Self::KeyNotFound => formatter.write_str("custody key does not exist"),
            Self::CorruptRecord(reason) => write!(formatter, "corrupt key record: {reason}"),
            Self::Io(error) => write!(formatter, "custody storage failure: {error}"),
            Self::Kms(error) => write!(formatter, "{error}"),
            Self::Sign(error) => write!(formatter, "{error}"),
            Self::StepUpRequired => {
                formatter.write_str("operation requires step-up evidence bound to the disclosure")
            }
            Self::StepUpOperationMismatch => {
                formatter.write_str("step-up evidence is bound to another operation")
            }
            Self::StepUpMismatch => {
                formatter.write_str("step-up evidence is bound to another disclosure")
            }
            Self::StepUpNotYetValid => formatter.write_str("step-up evidence is not valid yet"),
            Self::StepUpExpired => formatter.write_str("step-up evidence is outside its validity"),
            Self::StepUpReplayed => formatter.write_str("step-up evidence was already consumed"),
            Self::NonMonotonicTime => {
                formatter.write_str("signing time moved behind the active throughput window")
            }
            Self::ThroughputExceeded { retry_at } => write!(
                formatter,
                "per-principal signing throughput exceeded until {retry_at}"
            ),
            Self::CoordinationUnavailable => {
                formatter.write_str("custody signing coordination is unavailable")
            }
            Self::Audit(error) => {
                write!(formatter, "signing decision could not be audited: {error}")
            }
            Self::Store(error) => write!(formatter, "principal store refused: {error}"),
        }
    }
}

impl std::error::Error for CustodyError {}

impl From<io::Error> for CustodyError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<KmsError> for CustodyError {
    fn from(value: KmsError) -> Self {
        Self::Kms(value)
    }
}

impl From<SignError> for CustodyError {
    fn from(value: SignError) -> Self {
        Self::Sign(value)
    }
}

/// Public description of one held key: everything the keystore ever returns.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyDescriptor {
    /// The class the key signs for.
    pub class: KeyClass,
    /// The public verification key.
    pub public_key: [u8; 32],
}

/// Availability of one custody dependency on the status surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Availability {
    Available,
    Unavailable,
}

/// Aggregate integrity of principal-scoped provider key references.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyReferenceIntegrity {
    Verified,
    Failed,
    Unknown,
}

/// Degradation state surfaced for the custody service.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CustodyStatus {
    /// Whether the KMS boundary can be reached.
    pub kms: Availability,
    /// Whether the sealed-record storage can be reached.
    pub storage: Availability,
    /// Whether every persisted provider handle describes the expected key and
    /// principal binding.
    pub key_references: KeyReferenceIntegrity,
    /// Aggregate provider-reported key rotation state.
    pub rotation: RotationState,
}

impl CustodyStatus {
    /// Returns whether any custody dependency is degraded.
    #[must_use]
    pub const fn degraded(self) -> bool {
        !matches!(
            (self.kms, self.storage, self.key_references, self.rotation),
            (
                Availability::Available,
                Availability::Available,
                KeyReferenceIntegrity::Verified,
                RotationState::Stable
            )
        )
    }
}

struct KeyRecord {
    class: KeyClass,
    public_key: [u8; 32],
    binding_digest: Option<[u8; 32]>,
    provider_reference: ProviderKeyReference,
}

/// KMS-backed keystore holding only provider references and public key facts
/// under principal-scoped subtrees. Production private keys never enter this
/// process and no method returns private material.
#[derive(Debug)]
pub struct Keystore {
    root: PathBuf,
    network_id: u32,
    provider: Arc<dyn KmsProvider>,
}

impl Keystore {
    /// Opens the production keystore at `root` for one network under a remote
    /// KMS/HSM provider. Development-only providers are always refused.
    ///
    /// # Errors
    ///
    /// Refuses development providers, unavailable remote providers, network
    /// zero, foreign entries in the principals tree and storage failures.
    pub fn open<P>(
        root: impl AsRef<Path>,
        network_id: u32,
        provider: P,
    ) -> Result<Self, CustodyError>
    where
        P: KmsProvider + 'static,
    {
        Self::open_shared(root, network_id, Arc::new(provider), true)
    }

    /// Opens the explicitly development-only file-envelope keystore.
    ///
    /// # Errors
    ///
    /// Returns invalid-network, corrupt-tree and storage failures. This method
    /// cannot accept a production provider.
    pub fn open_development(
        root: impl AsRef<Path>,
        network_id: u32,
        provider: EnvelopeKms,
    ) -> Result<Self, CustodyError> {
        Self::open_shared(root, network_id, Arc::new(provider), false)
    }

    /// Opens a production keystore and refuses a development-only provider or
    /// an unreachable remote boundary before service startup completes.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal for a development provider, provider readiness
    /// failure, invalid network, corrupt custody tree or storage failure.
    pub fn open_production<P>(
        root: impl AsRef<Path>,
        network_id: u32,
        provider: P,
    ) -> Result<Self, CustodyError>
    where
        P: KmsProvider + 'static,
    {
        Self::open(root, network_id, provider)
    }

    fn open_shared(
        root: impl AsRef<Path>,
        network_id: u32,
        provider: Arc<dyn KmsProvider>,
        production: bool,
    ) -> Result<Self, CustodyError> {
        if network_id == 0 {
            return Err(CustodyError::InvalidNetwork);
        }
        let provider_reference = provider.provider_reference();
        if provider_reference.is_empty()
            || provider_reference.len() > KEY_REFERENCE_LIMIT
            || provider_reference.as_bytes().contains(&0)
        {
            return Err(CustodyError::InvalidKeyReference);
        }
        if production && provider.deployment() != ProviderDeployment::Production {
            return Err(CustodyError::DevelopmentProviderInProduction);
        }
        if production {
            provider.probe()?;
        }
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root)?;
        require_directory(&root, "custody root is not a directory")?;
        verify_custody_tree(&root)?;
        Ok(Self {
            root,
            network_id,
            provider,
        })
    }

    /// Returns the network every held key is bound to.
    #[must_use]
    pub const fn network_id(&self) -> u32 {
        self.network_id
    }

    /// Returns the non-secret KMS root key reference in use.
    #[must_use]
    pub fn key_reference(&self) -> &str {
        self.provider.provider_reference()
    }

    /// Creates a key wholly inside the selected provider and returns only its
    /// public verification key.
    ///
    /// # Errors
    ///
    /// Refuses duplicates and returns typed provider, integrity and storage
    /// failures without persisting a partial record.
    pub fn create(
        &self,
        principal: &PrincipalId,
        key: &KeyId,
        class: KeyClass,
    ) -> Result<[u8; 32], CustodyError> {
        self.create_with(principal, key, class, None)
    }

    /// Generates one key inside the KMS boundary and returns only its public
    /// verification key. The supplied entropy is zeroized on release.
    ///
    /// # Errors
    ///
    /// Refuses to overwrite an existing key and returns typed KMS and storage
    /// refusals with nothing persisted.
    pub fn generate(
        &self,
        principal: &PrincipalId,
        key: &KeyId,
        class: KeyClass,
        entropy: KeyEntropy,
    ) -> Result<[u8; 32], CustodyError> {
        if self.provider.deployment() != ProviderDeployment::DevelopmentOnly {
            return Err(CustodyError::Kms(KmsError::DevelopmentOnly));
        }
        self.create_with(principal, key, class, Some(entropy))
    }

    fn create_with(
        &self,
        principal: &PrincipalId,
        key: &KeyId,
        class: KeyClass,
        entropy: Option<KeyEntropy>,
    ) -> Result<[u8; 32], CustodyError> {
        let directory = self.create_principal_directory(principal)?;
        let path = directory.join(format!("{}{KEY_FILE_SUFFIX}", key.as_str()));
        if fs::symlink_metadata(&path).is_ok() {
            return Err(CustodyError::KeyExists);
        }
        let binding = self.binding(principal, key, class)?;
        let description = match entropy {
            Some(entropy) => self.provider.create_development_key(&binding, entropy)?,
            None => self.provider.create_key(&binding)?,
        };
        require_description(&binding, class, None, &description)?;
        let public_key = description.public_key();
        let record = encode_record(class, &description)?;
        write_atomic(
            &directory,
            &format!("{}{KEY_FILE_SUFFIX}", key.as_str()),
            &format!("{}{TEMP_FILE_SUFFIX}", key.as_str()),
            &record,
        )?;
        Ok(public_key)
    }

    /// Describes one held key and verifies its provider reference against the
    /// principal-scoped local record.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal for unknown keys and corrupt records.
    pub fn describe(
        &self,
        principal: &PrincipalId,
        key: &KeyId,
    ) -> Result<KeyDescriptor, CustodyError> {
        let record = self.read_record(principal, key)?;
        let binding = self.binding(principal, key, record.class)?;
        self.require_record_binding(&binding, &record)?;
        let description = self
            .provider
            .describe_key(&binding, &record.provider_reference)?;
        require_description(
            &binding,
            record.class,
            Some(record.public_key),
            &description,
        )?;
        if description.reference() != &record.provider_reference {
            return Err(CustodyError::Kms(KmsError::Integrity));
        }
        Ok(KeyDescriptor {
            class: record.class,
            public_key: description.public_key(),
        })
    }

    /// Rotates one key inside the provider and atomically replaces only its
    /// opaque handle and public facts in local storage.
    ///
    /// # Errors
    ///
    /// Returns typed provider, principal-binding and storage failures.
    pub fn rotate(
        &self,
        principal: &PrincipalId,
        key: &KeyId,
    ) -> Result<KeyDescriptor, CustodyError> {
        let record = self.read_record(principal, key)?;
        let binding = self.binding(principal, key, record.class)?;
        self.require_record_binding(&binding, &record)?;
        let description = self
            .provider
            .rotate_key(&binding, &record.provider_reference)?;
        require_description(&binding, record.class, None, &description)?;
        if self.provider.deployment() == ProviderDeployment::Production
            && description.reference() != &record.provider_reference
        {
            return Err(CustodyError::Kms(KmsError::Integrity));
        }
        let bytes = encode_record(record.class, &description)?;
        replace_atomic(
            &self.principal_directory(principal),
            &format!("{}{KEY_FILE_SUFFIX}", key.as_str()),
            &format!("{}{TEMP_FILE_SUFFIX}", key.as_str()),
            &bytes,
        )?;
        Ok(KeyDescriptor {
            class: record.class,
            public_key: description.public_key(),
        })
    }

    /// Destroys one provider key before removing its local opaque record.
    ///
    /// # Errors
    ///
    /// Returns typed provider, principal-binding and storage failures. A
    /// provider refusal never removes the local record.
    pub fn destroy(&self, principal: &PrincipalId, key: &KeyId) -> Result<(), CustodyError> {
        let record = self.read_record(principal, key)?;
        let binding = self.binding(principal, key, record.class)?;
        self.require_record_binding(&binding, &record)?;
        self.provider
            .destroy_key(&binding, &record.provider_reference)?;
        fs::remove_file(self.key_path(principal, key))?;
        fs::File::open(self.principal_directory(principal))?.sync_all()?;
        Ok(())
    }

    /// Lists one principal's key identifiers in deterministic order.
    ///
    /// # Errors
    ///
    /// Returns storage failures and refuses foreign entries in the subtree.
    pub fn keys(&self, principal: &PrincipalId) -> Result<Vec<KeyId>, CustodyError> {
        let directory = self.principal_directory(principal);
        if !directory.try_exists()? {
            return Ok(Vec::new());
        }
        require_directory(&directory, "principal key path is not a directory")?;
        let mut keys = Vec::new();
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                return Err(CustodyError::CorruptRecord("foreign key entry"));
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                return Err(CustodyError::CorruptRecord("foreign key entry"));
            };
            if name.ends_with(TEMP_FILE_SUFFIX) {
                continue;
            }
            let Some(stem) = name.strip_suffix(KEY_FILE_SUFFIX) else {
                return Err(CustodyError::CorruptRecord("foreign key entry"));
            };
            keys.push(
                KeyId::new(stem).map_err(|_| CustodyError::CorruptRecord("foreign key entry"))?,
            );
        }
        keys.sort();
        Ok(keys)
    }

    /// Reports KMS and storage availability for the status surface.
    #[must_use]
    pub fn status(&self) -> CustodyStatus {
        let kms = match self.provider.probe() {
            Ok(()) => Availability::Available,
            Err(_) => Availability::Unavailable,
        };
        let (storage, key_references, rotation) = match self.inspect_records() {
            Ok((key_references, rotation)) => (Availability::Available, key_references, rotation),
            Err(_) => (
                Availability::Unavailable,
                KeyReferenceIntegrity::Unknown,
                RotationState::Unknown,
            ),
        };
        CustodyStatus {
            kms,
            storage,
            key_references,
            rotation,
        }
    }

    pub(crate) fn remote_signer(
        &self,
        principal: &PrincipalId,
        key: &KeyId,
    ) -> Result<RemoteCustodySigner, CustodyError> {
        let record = self.read_record(principal, key)?;
        let binding = self.binding(principal, key, record.class)?;
        self.require_record_binding(&binding, &record)?;
        let description = self
            .provider
            .describe_key(&binding, &record.provider_reference)?;
        require_description(
            &binding,
            record.class,
            Some(record.public_key),
            &description,
        )?;
        if description.reference() != &record.provider_reference {
            return Err(CustodyError::Kms(KmsError::Integrity));
        }
        Ok(RemoteCustodySigner::new(
            Arc::clone(&self.provider),
            binding,
            description,
            record.class,
        ))
    }

    fn binding(
        &self,
        principal: &PrincipalId,
        key: &KeyId,
        class: KeyClass,
    ) -> Result<PrincipalKeyBinding, CustodyError> {
        PrincipalKeyBinding::new(
            identity_bytes(principal, key, class),
            self.network_id,
            class,
            self.provider.provider_reference(),
        )
    }

    fn require_record_binding(
        &self,
        binding: &PrincipalKeyBinding,
        record: &KeyRecord,
    ) -> Result<(), CustodyError> {
        if record
            .binding_digest
            .is_some_and(|digest| !layerx_crypto::ct::eq_fixed(&digest, &binding.digest()))
        {
            return Err(CustodyError::Kms(KmsError::Refused));
        }
        Ok(())
    }

    fn inspect_records(&self) -> Result<(KeyReferenceIntegrity, RotationState), CustodyError> {
        require_directory(&self.root, "custody root is not a directory")?;
        let principals = self.root.join(PRINCIPALS_DIR);
        if !principals.try_exists()? {
            return Ok((KeyReferenceIntegrity::Verified, RotationState::Stable));
        }
        require_directory(&principals, "principals path is not a directory")?;
        let mut integrity = KeyReferenceIntegrity::Verified;
        let mut rotation = RotationState::Stable;
        for principal_entry in fs::read_dir(principals)? {
            let principal_entry = principal_entry?;
            let Some(principal_name) = principal_entry.file_name().to_str().map(str::to_owned)
            else {
                integrity = KeyReferenceIntegrity::Failed;
                continue;
            };
            let Ok(principal) = PrincipalId::new(principal_name) else {
                integrity = KeyReferenceIntegrity::Failed;
                continue;
            };
            for key_entry in fs::read_dir(principal_entry.path())? {
                let key_entry = key_entry?;
                let Some(key_name) = key_entry.file_name().to_str().map(str::to_owned) else {
                    integrity = KeyReferenceIntegrity::Failed;
                    continue;
                };
                if key_name.ends_with(TEMP_FILE_SUFFIX) {
                    continue;
                }
                let Some(stem) = key_name.strip_suffix(KEY_FILE_SUFFIX) else {
                    integrity = KeyReferenceIntegrity::Failed;
                    continue;
                };
                let Ok(key) = KeyId::new(stem) else {
                    integrity = KeyReferenceIntegrity::Failed;
                    continue;
                };
                let record = match self.read_record(&principal, &key) {
                    Ok(record) => record,
                    Err(CustodyError::Io(error)) => return Err(CustodyError::Io(error)),
                    Err(_) => {
                        integrity = KeyReferenceIntegrity::Failed;
                        continue;
                    }
                };
                let binding = self.binding(&principal, &key, record.class)?;
                if self.require_record_binding(&binding, &record).is_err() {
                    integrity = KeyReferenceIntegrity::Failed;
                    continue;
                }
                match self
                    .provider
                    .describe_key(&binding, &record.provider_reference)
                {
                    Ok(description)
                        if require_description(
                            &binding,
                            record.class,
                            Some(record.public_key),
                            &description,
                        )
                        .is_ok()
                            && description.reference() == &record.provider_reference =>
                    {
                        rotation = merge_rotation(rotation, description.rotation());
                    }
                    Err(KmsError::Unavailable | KmsError::Timeout | KmsError::Authentication) => {
                        if integrity != KeyReferenceIntegrity::Failed {
                            integrity = KeyReferenceIntegrity::Unknown;
                        }
                        rotation = RotationState::Unknown;
                    }
                    Ok(_) | Err(_) => integrity = KeyReferenceIntegrity::Failed,
                }
            }
        }
        Ok((integrity, rotation))
    }

    fn principal_directory(&self, principal: &PrincipalId) -> PathBuf {
        self.root.join(PRINCIPALS_DIR).join(principal.as_str())
    }

    fn key_path(&self, principal: &PrincipalId, key: &KeyId) -> PathBuf {
        self.principal_directory(principal)
            .join(format!("{}{KEY_FILE_SUFFIX}", key.as_str()))
    }

    fn read_record(&self, principal: &PrincipalId, key: &KeyId) -> Result<KeyRecord, CustodyError> {
        let path = self.key_path(principal, key);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_file() => metadata,
            Ok(_) => return Err(CustodyError::CorruptRecord("foreign key entry")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(CustodyError::KeyNotFound);
            }
            Err(error) => return Err(CustodyError::Io(error)),
        };
        if metadata.len() == 0 {
            return Err(CustodyError::CorruptRecord("empty key record"));
        }
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(CustodyError::KeyNotFound)
            }
            Err(error) => return Err(CustodyError::Io(error)),
        };
        decode_record(&bytes)
    }

    fn create_principal_directory(&self, principal: &PrincipalId) -> Result<PathBuf, CustodyError> {
        let principals = self.root.join(PRINCIPALS_DIR);
        match fs::create_dir(&principals) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(CustodyError::Io(error)),
        }
        require_directory(&principals, "principals path is not a directory")?;
        let directory = principals.join(principal.as_str());
        match fs::create_dir(&directory) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(CustodyError::Io(error)),
        }
        require_directory(&directory, "principal key path is not a directory")?;
        Ok(directory)
    }
}

fn identity_bytes(principal: &PrincipalId, key: &KeyId, class: KeyClass) -> Vec<u8> {
    let principal = principal.as_str().as_bytes();
    let key = key.as_str().as_bytes();
    let class = class.label().as_bytes();
    let mut identity =
        Vec::with_capacity(IDENTITY_DOMAIN.len() + principal.len() + key.len() + class.len() + 3);
    identity.extend_from_slice(IDENTITY_DOMAIN);
    identity.push(0);
    identity.extend_from_slice(principal);
    identity.push(0);
    identity.extend_from_slice(key);
    identity.push(0);
    identity.extend_from_slice(class);
    identity
}

fn require_description(
    binding: &PrincipalKeyBinding,
    class: KeyClass,
    expected_public_key: Option<[u8; 32]>,
    description: &ProviderKeyDescription,
) -> Result<(), CustodyError> {
    if binding.class() != class
        || !layerx_crypto::ct::eq_fixed(&description.binding_digest(), &binding.digest())
        || expected_public_key.is_some_and(|public_key| {
            !layerx_crypto::ct::eq_fixed(&public_key, &description.public_key())
        })
    {
        return Err(CustodyError::Kms(KmsError::Integrity));
    }
    Ok(())
}

const fn merge_rotation(current: RotationState, next: RotationState) -> RotationState {
    match (current, next) {
        (RotationState::Failed, _) | (_, RotationState::Failed) => RotationState::Failed,
        (RotationState::Unknown, _) | (_, RotationState::Unknown) => RotationState::Unknown,
        (RotationState::InProgress, _) | (_, RotationState::InProgress) => {
            RotationState::InProgress
        }
        (RotationState::Stable, RotationState::Stable) => RotationState::Stable,
    }
}

fn encode_record(
    class: KeyClass,
    description: &ProviderKeyDescription,
) -> Result<Vec<u8>, CustodyError> {
    let reference = description.reference().as_bytes();
    let reference_length = u32::try_from(reference.len())
        .map_err(|_| CustodyError::CorruptRecord("provider reference exceeds encoding bounds"))?;
    let mut output = Vec::with_capacity(4 + 1 + 1 + 32 + 32 + 4 + reference.len());
    output.extend_from_slice(RECORD_MAGIC);
    output.push(RECORD_VERSION);
    output.push(class.code());
    output.extend_from_slice(&description.public_key());
    output.extend_from_slice(&description.binding_digest());
    output.extend_from_slice(&reference_length.to_be_bytes());
    output.extend_from_slice(reference);
    Ok(output)
}

fn decode_record(bytes: &[u8]) -> Result<KeyRecord, CustodyError> {
    let mut reader = RecordReader::new(bytes);
    if reader.take(4)? != RECORD_MAGIC {
        return Err(CustodyError::CorruptRecord("invalid record header"));
    }
    let version = reader.byte()?;
    if !matches!(version, LEGACY_RECORD_VERSION | RECORD_VERSION) {
        return Err(CustodyError::CorruptRecord("unknown record version"));
    }
    let class = KeyClass::from_code(reader.byte()?)?;
    let public_key = reader
        .take(32)?
        .try_into()
        .map_err(|_| CustodyError::CorruptRecord("truncated public key"))?;
    let binding_digest = if version == RECORD_VERSION {
        Some(
            reader
                .take(32)?
                .try_into()
                .map_err(|_| CustodyError::CorruptRecord("truncated key binding"))?,
        )
    } else {
        None
    };
    let reference_length = reader.length()?;
    let provider_reference = ProviderKeyReference::new(reader.take(reference_length)?.to_vec())
        .map_err(CustodyError::Kms)?;
    reader.finish()?;
    Ok(KeyRecord {
        class,
        public_key,
        binding_digest,
        provider_reference,
    })
}

struct RecordReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> RecordReader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], CustodyError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(CustodyError::CorruptRecord("record length overflow"))?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(CustodyError::CorruptRecord("truncated record"))?;
        self.offset = end;
        Ok(value)
    }

    fn byte(&mut self) -> Result<u8, CustodyError> {
        Ok(self.take(1)?[0])
    }

    fn length(&mut self) -> Result<usize, CustodyError> {
        let bytes: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| CustodyError::CorruptRecord("truncated record"))?;
        usize::try_from(u32::from_be_bytes(bytes))
            .map_err(|_| CustodyError::CorruptRecord("record length overflow"))
    }

    fn finish(self) -> Result<(), CustodyError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(CustodyError::CorruptRecord("trailing record bytes"))
        }
    }
}

fn verify_custody_tree(root: &Path) -> Result<(), CustodyError> {
    let principals = root.join(PRINCIPALS_DIR);
    if !principals.try_exists()? {
        return Ok(());
    }
    require_directory(&principals, "principals path is not a directory")?;
    for entry in fs::read_dir(principals)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return Err(CustodyError::CorruptRecord("foreign principals entry"));
        };
        if PrincipalId::new(name).is_err() || !entry.file_type()?.is_dir() {
            return Err(CustodyError::CorruptRecord("foreign principals entry"));
        }
        for key_entry in fs::read_dir(entry.path())? {
            let key_entry = key_entry?;
            if !key_entry.file_type()?.is_file() {
                return Err(CustodyError::CorruptRecord("foreign key entry"));
            }
            let key_name = key_entry.file_name();
            let Some(key_name) = key_name.to_str() else {
                return Err(CustodyError::CorruptRecord("foreign key entry"));
            };
            let stem = key_name
                .strip_suffix(TEMP_FILE_SUFFIX)
                .or_else(|| key_name.strip_suffix(KEY_FILE_SUFFIX));
            let valid = stem.is_some_and(valid_identifier);
            if !valid {
                return Err(CustodyError::CorruptRecord("foreign key entry"));
            }
        }
    }
    Ok(())
}

fn require_directory(path: &Path, reason: &'static str) -> Result<(), CustodyError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_dir() {
        Ok(())
    } else {
        Err(CustodyError::CorruptRecord(reason))
    }
}

fn write_atomic(
    directory: &Path,
    final_name: &str,
    temp_name: &str,
    bytes: &[u8],
) -> Result<(), CustodyError> {
    use std::io::Write as _;

    let temp_path = directory.join(temp_name);
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp_path)?;
    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(&temp_path);
        return Err(CustodyError::Io(error));
    }
    drop(file);
    let final_path = directory.join(final_name);
    if let Err(error) = fs::hard_link(&temp_path, &final_path) {
        let _ = fs::remove_file(&temp_path);
        return if error.kind() == io::ErrorKind::AlreadyExists {
            Err(CustodyError::KeyExists)
        } else {
            Err(CustodyError::Io(error))
        };
    }
    fs::remove_file(temp_path)?;
    fs::File::open(directory)?.sync_all()?;
    Ok(())
}

fn replace_atomic(
    directory: &Path,
    final_name: &str,
    temp_name: &str,
    bytes: &[u8],
) -> Result<(), CustodyError> {
    use std::io::Write as _;

    let temp_path = directory.join(temp_name);
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp_path)?;
    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(&temp_path);
        return Err(CustodyError::Io(error));
    }
    drop(file);
    if let Err(error) = fs::rename(&temp_path, directory.join(final_name)) {
        let _ = fs::remove_file(&temp_path);
        return Err(CustodyError::Io(error));
    }
    fs::File::open(directory)?.sync_all()?;
    Ok(())
}
