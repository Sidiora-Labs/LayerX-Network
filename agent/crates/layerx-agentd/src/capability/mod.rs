//! Deterministic, explicitly bounded daemon capability model.

use std::collections::BTreeSet;

use crate::store::{ObjectKind, Store, StoreError, TenantId, TenantKey};

#[path = "attenuate.rs"]
mod attenuation;
#[path = "consume.rs"]
mod consumption;
mod narrowing;
#[path = "report.rs"]
mod reporting;

use crate::identity::ProtocolAuthority;
pub use attenuation::{AttenuationError, CapabilityGraph, RevocableActivity, RevocationResult};
pub use consumption::{
    Ceiling, CeilingError, CeilingSnapshot, ReceiptApplication, Reservation,
};
pub use narrowing::{Binding, Enforcement, NarrowingError, NarrowingReport, ProtocolScope};
pub use reporting::{
    check_guarantee_wording, CapabilityReport, DecisionEvidence, ReportError, ReportSurfaces,
    RestrictionReport,
};

/// Stable capability identifier.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CapabilityId(pub [u8; 32]);

/// Exact rate ceiling measured in protocol sequence windows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RateCeiling {
    pub maximum_uses: u64,
    pub window_sequences: u64,
}

/// All mandatory dimensions of one capability.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityDimensions {
    pub activity_types: BTreeSet<u16>,
    pub counterparties: BTreeSet<[u8; 32]>,
    pub assets: BTreeSet<[u8; 32]>,
    pub amount_ceiling: u128,
    pub rate_ceiling: RateCeiling,
    pub purposes: BTreeSet<String>,
    pub expiry_sequence: u64,
}

/// Tenant-owned capability with no implicit open dimension.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Capability {
    pub id: CapabilityId,
    pub tenant: TenantId,
    pub dimensions: CapabilityDimensions,
}

impl Capability {
    /// Constructs a capability only when every dimension is explicit and bounded.
    ///
    /// # Errors
    ///
    /// Names the first dimension left empty, zero, or otherwise unbounded.
    pub fn new(
        id: CapabilityId,
        tenant: TenantId,
        dimensions: CapabilityDimensions,
    ) -> Result<Self, CapabilityError> {
        if dimensions.activity_types.is_empty() {
            return Err(CapabilityError::MissingDimension(Dimension::ActivityType));
        }
        if dimensions.counterparties.is_empty() {
            return Err(CapabilityError::MissingDimension(Dimension::Counterparty));
        }
        if dimensions.assets.is_empty() {
            return Err(CapabilityError::MissingDimension(Dimension::Asset));
        }
        if dimensions.amount_ceiling == 0 {
            return Err(CapabilityError::MissingDimension(Dimension::Amount));
        }
        if dimensions.rate_ceiling.maximum_uses == 0
            || dimensions.rate_ceiling.window_sequences == 0
        {
            return Err(CapabilityError::MissingDimension(Dimension::Rate));
        }
        if dimensions.purposes.is_empty() || dimensions.purposes.iter().any(String::is_empty) {
            return Err(CapabilityError::MissingDimension(Dimension::Purpose));
        }
        if dimensions.expiry_sequence == 0 {
            return Err(CapabilityError::MissingDimension(Dimension::Expiry));
        }
        Ok(Self {
            id,
            tenant,
            dimensions,
        })
    }

    /// Persists the capability under its inseparable tenant key.
    ///
    /// # Errors
    ///
    /// Returns `SizeOverflow` when a dimension set or purpose exceeds its length prefix, and the
    /// store failure raised while writing the encoded object durably.
    pub fn persist(&self, store: &mut Store) -> Result<(), CapabilityError> {
        let key = TenantKey::new(
            self.tenant.clone(),
            ObjectKind::Capability,
            self.id.0.to_vec(),
        )?;
        store.put_local(key, encode(self)?)?;
        Ok(())
    }

    /// Restores a capability from the exact tenant-scoped key.
    ///
    /// # Errors
    ///
    /// Returns `Corrupt` for truncated, trailing, or non-UTF-8 stored bytes, and names the rejected
    /// tenant or unbounded dimension recovered from an otherwise decodable record.
    pub fn restore(
        store: &Store,
        tenant: TenantId,
        id: CapabilityId,
    ) -> Result<Option<Self>, CapabilityError> {
        let key = TenantKey::new(tenant, ObjectKind::Capability, id.0.to_vec())?;
        store
            .get(&key)
            .map(|value| decode(value.bytes()))
            .transpose()
    }
}

/// Prepared activity fields consumed by deterministic capability evaluation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedIntent {
    pub activity_type: u16,
    pub counterparty: [u8; 32],
    pub asset: [u8; 32],
    pub amount: u128,
    pub purpose: String,
    pub core_sequence: u64,
    pub uses_in_window: u64,
}

/// Stable first-failing dimension order.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Dimension {
    Expiry,
    ActivityType,
    Counterparty,
    Asset,
    Amount,
    Rate,
    Purpose,
}

/// Capability decision with the exceeded dimension named.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Decision {
    Allow,
    Refuse(Dimension),
}

/// Construction, storage, or decoding failure.
#[derive(Debug)]
pub enum CapabilityError {
    MissingDimension(Dimension),
    Store(StoreError),
    Corrupt,
    SizeOverflow,
}

impl From<StoreError> for CapabilityError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

/// Evaluates every dimension in a stable order with no ambient input.
#[must_use]
pub fn evaluate(capability: &Capability, intent: &PreparedIntent) -> Decision {
    let dimensions = &capability.dimensions;
    if intent.core_sequence >= dimensions.expiry_sequence {
        return Decision::Refuse(Dimension::Expiry);
    }
    if !dimensions.activity_types.contains(&intent.activity_type) {
        return Decision::Refuse(Dimension::ActivityType);
    }
    if !dimensions.counterparties.contains(&intent.counterparty) {
        return Decision::Refuse(Dimension::Counterparty);
    }
    if !dimensions.assets.contains(&intent.asset) {
        return Decision::Refuse(Dimension::Asset);
    }
    if intent.amount > dimensions.amount_ceiling {
        return Decision::Refuse(Dimension::Amount);
    }
    if intent.uses_in_window >= dimensions.rate_ceiling.maximum_uses {
        return Decision::Refuse(Dimension::Rate);
    }
    if !dimensions.purposes.contains(&intent.purpose) {
        return Decision::Refuse(Dimension::Purpose);
    }
    Decision::Allow
}

/// Proves that a daemon capability is no wider than its protocol authority.
///
/// # Errors
///
/// Names the first dimension whose bound exceeds the granted protocol scope.
pub fn assert_narrowing(
    capability: &Capability,
    authority: ProtocolAuthority,
    protocol_scope: &ProtocolScope,
) -> Result<Binding, NarrowingError> {
    narrowing::check_narrowing(capability, authority, protocol_scope)
}

/// Derives a child capability while preserving its chain to the root.
///
/// # Errors
///
/// Refuses an unknown parent, a foreign tenant, and a duplicate child identifier, then names the
/// first child dimension wider than the parent.
pub fn attenuate(
    graph: &mut CapabilityGraph,
    parent: CapabilityId,
    child: Capability,
) -> Result<(), AttenuationError> {
    attenuation::derive(graph, parent, child)
}

/// Revokes one capability and every descendant, cancelling only unsubmitted work.
///
/// # Errors
///
/// Returns `MissingParent` when the named root is absent from the graph.
pub fn revoke_subtree(
    graph: &mut CapabilityGraph,
    root: CapabilityId,
    activities: &mut [RevocableActivity],
) -> Result<RevocationResult, AttenuationError> {
    attenuation::revoke(graph, root, activities)
}

/// Serialises one reservation decision against a capability ceiling.
///
/// # Errors
///
/// Refuses a zero or already expired amount, an empty reservation or expected
/// activity identity, an unreconciled or poisoned ceiling, a duplicate
/// reservation identifier, an arithmetic overflow, and a total that would
/// exceed the maximum.
pub fn consume(
    ceiling: &Ceiling,
    reservation_id: [u8; 32],
    expected_activity_id: [u8; 32],
    amount: u128,
    expiry_sequence: u64,
    current_sequence: u64,
) -> Result<Reservation, CeilingError> {
    consumption::reserve(
        ceiling,
        reservation_id,
        expected_activity_id,
        amount,
        expiry_sequence,
        current_sequence,
    )
}

/// Builds one byte-identical report for contract, CLI and audit surfaces.
///
/// # Errors
///
/// Returns `Incomplete` when a restriction is missing or the derivation chain does not end at the
/// bound capability, and `MisleadingWording` when a statement overstates enforcement.
pub fn enforcement_report(
    binding: &Binding,
    derivation_chain: Vec<CapabilityId>,
    decision: DecisionEvidence,
) -> Result<ReportSurfaces, ReportError> {
    reporting::build_report(binding, derivation_chain, decision)
}

pub(crate) fn encode(capability: &Capability) -> Result<Vec<u8>, CapabilityError> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&capability.id.0);
    push_string(&mut bytes, capability.tenant.as_str())?;
    push_len(&mut bytes, capability.dimensions.activity_types.len())?;
    for value in &capability.dimensions.activity_types {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    push_len(&mut bytes, capability.dimensions.counterparties.len())?;
    for value in &capability.dimensions.counterparties {
        bytes.extend_from_slice(value);
    }
    push_len(&mut bytes, capability.dimensions.assets.len())?;
    for value in &capability.dimensions.assets {
        bytes.extend_from_slice(value);
    }
    bytes.extend_from_slice(&capability.dimensions.amount_ceiling.to_be_bytes());
    bytes.extend_from_slice(
        &capability
            .dimensions
            .rate_ceiling
            .maximum_uses
            .to_be_bytes(),
    );
    bytes.extend_from_slice(
        &capability
            .dimensions
            .rate_ceiling
            .window_sequences
            .to_be_bytes(),
    );
    push_len(&mut bytes, capability.dimensions.purposes.len())?;
    for purpose in &capability.dimensions.purposes {
        push_string(&mut bytes, purpose)?;
    }
    bytes.extend_from_slice(&capability.dimensions.expiry_sequence.to_be_bytes());
    Ok(bytes)
}

pub(crate) fn decode(bytes: &[u8]) -> Result<Capability, CapabilityError> {
    let mut decoder = Decoder { bytes, offset: 0 };
    let mut id = [0_u8; 32];
    id.copy_from_slice(decoder.take(32)?);
    let tenant = TenantId::new(decoder.string()?)?;
    let mut activity_types = BTreeSet::new();
    for _ in 0..decoder.len()? {
        activity_types.insert(decoder.u16()?);
    }
    let mut counterparties = BTreeSet::new();
    for _ in 0..decoder.len()? {
        let mut value = [0_u8; 32];
        value.copy_from_slice(decoder.take(32)?);
        counterparties.insert(value);
    }
    let mut assets = BTreeSet::new();
    for _ in 0..decoder.len()? {
        let mut value = [0_u8; 32];
        value.copy_from_slice(decoder.take(32)?);
        assets.insert(value);
    }
    let amount_ceiling = decoder.u128()?;
    let maximum_uses = decoder.u64()?;
    let window_sequences = decoder.u64()?;
    let mut purposes = BTreeSet::new();
    for _ in 0..decoder.len()? {
        purposes.insert(decoder.string()?);
    }
    let expiry_sequence = decoder.u64()?;
    if decoder.offset != bytes.len() {
        return Err(CapabilityError::Corrupt);
    }
    Capability::new(
        CapabilityId(id),
        tenant,
        CapabilityDimensions {
            activity_types,
            counterparties,
            assets,
            amount_ceiling,
            rate_ceiling: RateCeiling {
                maximum_uses,
                window_sequences,
            },
            purposes,
            expiry_sequence,
        },
    )
}

fn push_len(bytes: &mut Vec<u8>, value: usize) -> Result<(), CapabilityError> {
    let value = u16::try_from(value).map_err(|_| CapabilityError::SizeOverflow)?;
    bytes.extend_from_slice(&value.to_be_bytes());
    Ok(())
}

fn push_string(bytes: &mut Vec<u8>, value: &str) -> Result<(), CapabilityError> {
    push_len(bytes, value.len())?;
    bytes.extend_from_slice(value.as_bytes());
    Ok(())
}

struct Decoder<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Decoder<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], CapabilityError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(CapabilityError::Corrupt)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(CapabilityError::Corrupt)?;
        self.offset = end;
        Ok(value)
    }

    fn u16(&mut self) -> Result<u16, CapabilityError> {
        let mut value = [0_u8; 2];
        value.copy_from_slice(self.take(2)?);
        Ok(u16::from_be_bytes(value))
    }

    fn u64(&mut self) -> Result<u64, CapabilityError> {
        let mut value = [0_u8; 8];
        value.copy_from_slice(self.take(8)?);
        Ok(u64::from_be_bytes(value))
    }

    fn u128(&mut self) -> Result<u128, CapabilityError> {
        let mut value = [0_u8; 16];
        value.copy_from_slice(self.take(16)?);
        Ok(u128::from_be_bytes(value))
    }

    fn len(&mut self) -> Result<usize, CapabilityError> {
        Ok(usize::from(self.u16()?))
    }

    fn string(&mut self) -> Result<String, CapabilityError> {
        let length = self.len()?;
        let value =
            std::str::from_utf8(self.take(length)?).map_err(|_| CapabilityError::Corrupt)?;
        Ok(value.to_owned())
    }
}
