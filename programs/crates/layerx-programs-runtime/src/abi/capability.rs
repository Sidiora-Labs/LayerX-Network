//! Capability grants, canonical encoding, and downward-only narrowing.

use std::collections::BTreeMap;

use crate::accounts::{derive_program_account, MAX_PROGRAM_ACCOUNT_SEED_BYTES};
use crate::storage::ProgramId;

use super::{AbiError, MAX_CAPABILITIES};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum CapabilityKey {
    StorageRead,
    StorageWrite,
    EmitEvent,
    Call(ProgramId),
    Transfer {
        asset: [u8; 32],
        to: [u8; 32],
    },
    ProgramSpend {
        owner_program: ProgramId,
        seed: Vec<u8>,
        source_account: [u8; 32],
        asset: [u8; 32],
        to: [u8; 32],
    },
    ReceiptRead([u8; 32]),
    BalanceView {
        account: [u8; 32],
        asset: [u8; 32],
    },
    SharedStorageRead,
    SharedStorageWrite,
}

/// One explicit authority granted by the invoking activity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Capability {
    StorageRead,
    StorageWrite,
    SharedStorageRead,
    SharedStorageWrite,
    EmitEvent,
    Call {
        program: ProgramId,
    },
    Transfer402 {
        asset: [u8; 32],
        to: [u8; 32],
        maximum_amount: u128,
    },
    ProgramSpend {
        owner_program: ProgramId,
        seed: Vec<u8>,
        source_account: [u8; 32],
        asset: [u8; 32],
        to: [u8; 32],
        maximum_amount: u128,
    },
    ReceiptRead {
        receipt_digest: [u8; 32],
    },
    /// Sight of one exact account/asset fact at one verified receipt. This
    /// grant is never consulted by either spending-authority path.
    BalanceView {
        account: [u8; 32],
        asset: [u8; 32],
        receipt_digest: [u8; 32],
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ProgramSpendAuthorization<'a> {
    pub staging_program: ProgramId,
    pub owner_program: ProgramId,
    pub seed: &'a [u8],
    pub source_account: [u8; 32],
    pub asset: [u8; 32],
    pub to: [u8; 32],
    pub amount: u128,
}

impl Capability {
    fn key(&self) -> CapabilityKey {
        match self {
            Self::StorageRead => CapabilityKey::StorageRead,
            Self::StorageWrite => CapabilityKey::StorageWrite,
            Self::SharedStorageRead => CapabilityKey::SharedStorageRead,
            Self::SharedStorageWrite => CapabilityKey::SharedStorageWrite,
            Self::EmitEvent => CapabilityKey::EmitEvent,
            Self::Call { program } => CapabilityKey::Call(*program),
            Self::Transfer402 { asset, to, .. } => CapabilityKey::Transfer {
                asset: *asset,
                to: *to,
            },
            Self::ProgramSpend {
                owner_program,
                seed,
                source_account,
                asset,
                to,
                ..
            } => CapabilityKey::ProgramSpend {
                owner_program: *owner_program,
                seed: seed.clone(),
                source_account: *source_account,
                asset: *asset,
                to: *to,
            },
            Self::ReceiptRead { receipt_digest } => CapabilityKey::ReceiptRead(*receipt_digest),
            Self::BalanceView { account, asset, .. } => CapabilityKey::BalanceView {
                account: *account,
                asset: *asset,
            },
        }
    }

    fn valid(&self) -> bool {
        match self {
            Self::Transfer402 {
                asset,
                to,
                maximum_amount,
            } => asset != &[0; 32] && to != &[0; 32] && *maximum_amount != 0,
            Self::ProgramSpend {
                owner_program,
                seed,
                source_account,
                asset,
                to,
                maximum_amount,
            } => {
                seed.len() <= MAX_PROGRAM_ACCOUNT_SEED_BYTES
                    && asset != &[0; 32]
                    && to != &[0; 32]
                    && *maximum_amount != 0
                    && derive_program_account(*owner_program, seed)
                        .is_ok_and(|derived| derived.matches(source_account))
            }
            Self::ReceiptRead { receipt_digest } => receipt_digest != &[0; 32],
            Self::BalanceView {
                account,
                asset,
                receipt_digest,
            } => account != &[0; 32] && asset != &[0; 32] && receipt_digest != &[0; 32],
            Self::StorageRead
            | Self::StorageWrite
            | Self::SharedStorageRead
            | Self::SharedStorageWrite
            | Self::EmitEvent
            | Self::Call { .. } => true,
        }
    }
}

/// Closed set of explicit capabilities. Duplicate authority keys are refused,
/// preventing ambiguous limits.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CapabilitySet(BTreeMap<CapabilityKey, Capability>);

impl CapabilitySet {
    pub(crate) fn reachable_accesses(
        &self,
        program: ProgramId,
        principal: crate::PrincipalId,
    ) -> Result<crate::AccessSet, crate::AccessRefusal> {
        let mut storage = std::collections::BTreeSet::new();
        let mut accounts = std::collections::BTreeSet::new();
        let mut callees = std::collections::BTreeSet::new();
        let mut reachable_programs = std::collections::BTreeSet::from([program]);
        for capability in self.0.values() {
            if let Capability::Call { program } = capability {
                reachable_programs.insert(*program);
            }
        }
        for capability in self.0.values() {
            match capability {
                Capability::StorageRead => {
                    for reachable in &reachable_programs { storage.insert(crate::StorageAccess::new(crate::StorageNamespace::principal(*reachable, principal), crate::AccessMode::Read, crate::KeyAccess::prefix([])?)?); }
                }
                Capability::StorageWrite => {
                    for reachable in &reachable_programs { storage.insert(crate::StorageAccess::new(crate::StorageNamespace::principal(*reachable, principal), crate::AccessMode::Write, crate::KeyAccess::prefix([])?)?); }
                }
                Capability::SharedStorageRead => {
                    for reachable in &reachable_programs { storage.insert(crate::StorageAccess::new(crate::StorageNamespace::shared(*reachable), crate::AccessMode::Read, crate::KeyAccess::prefix([])?)?); }
                }
                Capability::SharedStorageWrite => {
                    for reachable in &reachable_programs { storage.insert(crate::StorageAccess::new(crate::StorageNamespace::shared(*reachable), crate::AccessMode::Write, crate::KeyAccess::prefix([])?)?); }
                }
                Capability::Transfer402 { asset, to, .. } => {
                    accounts.insert(crate::AccountAccess::new(principal.bytes(), *asset, crate::AccessMode::Write)?);
                    accounts.insert(crate::AccountAccess::new(*to, *asset, crate::AccessMode::Write)?);
                }
                Capability::ProgramSpend { source_account, asset, to, .. } => {
                    accounts.insert(crate::AccountAccess::new(*source_account, *asset, crate::AccessMode::Write)?);
                    accounts.insert(crate::AccountAccess::new(*to, *asset, crate::AccessMode::Write)?);
                }
                Capability::BalanceView { account, asset, .. } => {
                    accounts.insert(crate::AccountAccess::new(*account, *asset, crate::AccessMode::Read)?);
                }
                Capability::Call { program } => { callees.insert(*program); }
                Capability::EmitEvent | Capability::ReceiptRead { .. } => {}
            }
        }
        crate::AccessSet::new_with_callees(storage, accounts, callees)
    }

    /// Constructs a validated capability set.
    ///
    /// # Errors
    ///
    /// Refuses invalid or duplicate grants.
    pub fn new(grants: impl IntoIterator<Item = Capability>) -> Result<Self, AbiError> {
        let mut capabilities = BTreeMap::new();
        let mut balance_views = 0usize;
        for grant in grants {
            if capabilities.len() == MAX_CAPABILITIES {
                return Err(AbiError::InvalidCapability);
            }
            if !grant.valid() {
                return Err(AbiError::InvalidCapability);
            }
            if matches!(grant, Capability::BalanceView { .. }) {
                balance_views = balance_views.saturating_add(1);
                if balance_views > super::MAX_BALANCE_VIEW_GRANTS {
                    return Err(AbiError::InvalidCapability);
                }
            }
            if capabilities.insert(grant.key(), grant).is_some() {
                return Err(AbiError::DuplicateCapability);
            }
        }
        Ok(Self(capabilities))
    }

    /// Encodes this set into the frozen deterministic capability-list format
    /// consumed by `program_call`.
    #[must_use]
    pub fn canonical_encoding(&self) -> Vec<u8> {
        let count = u16::try_from(self.0.len()).unwrap_or(u16::MAX);
        let mut encoded = Vec::with_capacity(2 + self.0.len().saturating_mul(81));
        encoded.extend_from_slice(&count.to_be_bytes());
        for capability in self.0.values() {
            match capability {
                Capability::StorageRead => encoded.push(1),
                Capability::StorageWrite => encoded.push(2),
                Capability::SharedStorageRead => encoded.push(7),
                Capability::SharedStorageWrite => encoded.push(8),
                Capability::EmitEvent => encoded.push(3),
                Capability::Call { program } => {
                    encoded.push(4);
                    encoded.extend_from_slice(&program.bytes());
                }
                Capability::Transfer402 {
                    asset,
                    to,
                    maximum_amount,
                } => {
                    encoded.push(5);
                    encoded.extend_from_slice(asset);
                    encoded.extend_from_slice(to);
                    encoded.extend_from_slice(&maximum_amount.to_be_bytes());
                }
                Capability::ProgramSpend {
                    owner_program,
                    seed,
                    source_account,
                    asset,
                    to,
                    maximum_amount,
                } => {
                    encoded.push(9);
                    encoded.extend_from_slice(&owner_program.bytes());
                    let seed_length = u16::try_from(seed.len()).unwrap_or(u16::MAX);
                    encoded.extend_from_slice(&seed_length.to_be_bytes());
                    encoded.extend_from_slice(seed);
                    encoded.extend_from_slice(source_account);
                    encoded.extend_from_slice(asset);
                    encoded.extend_from_slice(to);
                    encoded.extend_from_slice(&maximum_amount.to_be_bytes());
                }
                Capability::ReceiptRead { receipt_digest } => {
                    encoded.push(6);
                    encoded.extend_from_slice(receipt_digest);
                }
                Capability::BalanceView {
                    account,
                    asset,
                    receipt_digest,
                } => {
                    encoded.push(10);
                    encoded.extend_from_slice(account);
                    encoded.extend_from_slice(asset);
                    encoded.extend_from_slice(receipt_digest);
                }
            }
        }
        encoded
    }

    /// Returns an empty ambient-authority-free set.
    #[must_use]
    pub const fn empty() -> Self {
        Self(BTreeMap::new())
    }

    /// Narrows this authority to an explicitly requested subset.
    ///
    /// # Errors
    ///
    /// Refuses every missing grant or increased transfer limit.
    pub fn narrow(
        &self,
        requested: impl IntoIterator<Item = Capability>,
    ) -> Result<Self, AbiError> {
        self.narrow_with_origin(requested, None)
    }

    pub(crate) fn narrow_for_program_edge(
        &self,
        caller_program: ProgramId,
        requested: impl IntoIterator<Item = Capability>,
    ) -> Result<Self, AbiError> {
        self.narrow_with_origin(requested, Some(caller_program))
    }

    fn narrow_with_origin(
        &self,
        requested: impl IntoIterator<Item = Capability>,
        originating_program: Option<ProgramId>,
    ) -> Result<Self, AbiError> {
        let narrowed = Self::new(requested)?;
        for (key, request) in &narrowed.0 {
            if matches!(
                request,
                Capability::ProgramSpend { owner_program, .. }
                    if Some(*owner_program) == originating_program
            ) {
                continue;
            }
            let Some(parent) = self.0.get(key) else {
                return if matches!(request, Capability::ProgramSpend { .. }) {
                    Err(AbiError::CapabilityEscalation)
                } else {
                    Err(AbiError::CapabilityDenied)
                };
            };
            match (request, parent) {
                (
                    Capability::Transfer402 {
                        maximum_amount: requested,
                        ..
                    },
                    Capability::Transfer402 {
                        maximum_amount: granted,
                        ..
                    },
                )
                | (
                    Capability::ProgramSpend {
                        maximum_amount: requested,
                        ..
                    },
                    Capability::ProgramSpend {
                        maximum_amount: granted,
                        ..
                    },
                ) if requested > granted => return Err(AbiError::CapabilityEscalation),
                (
                    Capability::BalanceView {
                        receipt_digest: requested,
                        ..
                    },
                    Capability::BalanceView {
                        receipt_digest: granted,
                        ..
                    },
                ) if requested != granted => return Err(AbiError::CapabilityEscalation),
                _ => {}
            }
        }
        Ok(narrowed)
    }

    pub(crate) fn root_program_spend_is_owned_by(&self, program: ProgramId) -> bool {
        self.0.values().all(|capability| {
            !matches!(
                capability,
                Capability::ProgramSpend { owner_program, .. } if *owner_program != program
            )
        })
    }

    pub(crate) fn has_program_spend(&self) -> bool {
        self.0
            .values()
            .any(|capability| matches!(capability, Capability::ProgramSpend { .. }))
    }

    pub(crate) fn has_v2_only_grant(&self) -> bool {
        self.0.values().any(|capability| {
            matches!(
                capability,
                Capability::ProgramSpend { .. } | Capability::BalanceView { .. }
            )
        })
    }

    pub(super) fn grant(&self, key: &CapabilityKey) -> Result<&Capability, AbiError> {
        self.0.get(key).ok_or(AbiError::CapabilityDenied)
    }

    pub(crate) fn permits_transfer(&self, asset: [u8; 32], to: [u8; 32], amount: u128) -> bool {
        matches!(
            self.0.get(&CapabilityKey::Transfer { asset, to }),
            Some(Capability::Transfer402 { maximum_amount, .. }) if amount <= *maximum_amount
        )
    }

    pub(crate) fn permits_program_spend(
        &self,
        authorization: ProgramSpendAuthorization<'_>,
    ) -> bool {
        if authorization.amount == 0 || authorization.staging_program != authorization.owner_program
        {
            return false;
        }
        if !derive_program_account(authorization.owner_program, authorization.seed)
            .is_ok_and(|derived| derived.matches(&authorization.source_account))
        {
            return false;
        }
        let key = CapabilityKey::ProgramSpend {
            owner_program: authorization.owner_program,
            seed: authorization.seed.to_vec(),
            source_account: authorization.source_account,
            asset: authorization.asset,
            to: authorization.to,
        };
        matches!(
            self.0.get(&key),
            Some(Capability::ProgramSpend { maximum_amount, .. })
                if authorization.amount <= *maximum_amount
        )
    }

    pub(crate) fn contains_narrowed_for_program_edge(
        &self,
        caller_program: ProgramId,
        requested: &Self,
    ) -> bool {
        self.contains_narrowed_with_origin(requested, Some(caller_program))
    }

    fn contains_narrowed_with_origin(
        &self,
        requested: &Self,
        originating_program: Option<ProgramId>,
    ) -> bool {
        requested.0.iter().all(|(key, request)| {
            if matches!(
                request,
                Capability::ProgramSpend { owner_program, .. }
                    if Some(*owner_program) == originating_program
            ) {
                return true;
            }
            match (self.0.get(key), request) {
                (
                    Some(Capability::Transfer402 {
                        maximum_amount: parent,
                        ..
                    }),
                    Capability::Transfer402 {
                        maximum_amount: child,
                        ..
                    },
                ) => child <= parent,
                (
                    Some(Capability::ProgramSpend {
                        maximum_amount: parent,
                        ..
                    }),
                    Capability::ProgramSpend {
                        maximum_amount: child,
                        ..
                    },
                ) => child <= parent,
                (
                    Some(Capability::BalanceView {
                        receipt_digest: parent,
                        ..
                    }),
                    Capability::BalanceView {
                        receipt_digest: child,
                        ..
                    },
                ) => child == parent,
                (Some(_), _) => true,
                (None, _) => false,
            }
        })
    }

    pub(crate) fn decode_canonical(bytes: &[u8]) -> Result<Vec<Capability>, AbiError> {
        Self::decode_versioned_canonical(bytes, false)
    }

    pub(crate) fn decode_candidate_canonical(bytes: &[u8]) -> Result<Vec<Capability>, AbiError> {
        Self::decode_versioned_canonical(bytes, true)
    }

    fn decode_versioned_canonical(
        bytes: &[u8],
        candidate_v2: bool,
    ) -> Result<Vec<Capability>, AbiError> {
        if bytes.len() < 2 {
            return Err(AbiError::InvalidEncoding);
        }
        let count = usize::from(u16::from_be_bytes([bytes[0], bytes[1]]));
        if count > MAX_CAPABILITIES {
            return Err(AbiError::InvalidEncoding);
        }
        let mut cursor = 2usize;
        let mut grants = Vec::with_capacity(count);
        for _ in 0..count {
            let tag = *bytes.get(cursor).ok_or(AbiError::InvalidEncoding)?;
            cursor = cursor.checked_add(1).ok_or(AbiError::InvalidEncoding)?;
            let grant = match tag {
                1 => Capability::StorageRead,
                2 => Capability::StorageWrite,
                7 => Capability::SharedStorageRead,
                8 => Capability::SharedStorageWrite,
                3 => Capability::EmitEvent,
                4 => Capability::Call {
                    program: ProgramId::new(take_array::<32>(bytes, &mut cursor)?)?,
                },
                5 => Capability::Transfer402 {
                    asset: take_array::<32>(bytes, &mut cursor)?,
                    to: take_array::<32>(bytes, &mut cursor)?,
                    maximum_amount: u128::from_be_bytes(take_array::<16>(bytes, &mut cursor)?),
                },
                6 => Capability::ReceiptRead {
                    receipt_digest: take_array::<32>(bytes, &mut cursor)?,
                },
                9 if candidate_v2 => {
                    let owner_program = ProgramId::new(take_array::<32>(bytes, &mut cursor)?)?;
                    let seed_length =
                        usize::from(u16::from_be_bytes(take_array::<2>(bytes, &mut cursor)?));
                    if seed_length > MAX_PROGRAM_ACCOUNT_SEED_BYTES {
                        return Err(AbiError::InvalidEncoding);
                    }
                    let seed = take_slice(bytes, &mut cursor, seed_length)?.to_vec();
                    Capability::ProgramSpend {
                        owner_program,
                        seed,
                        source_account: take_array::<32>(bytes, &mut cursor)?,
                        asset: take_array::<32>(bytes, &mut cursor)?,
                        to: take_array::<32>(bytes, &mut cursor)?,
                        maximum_amount: u128::from_be_bytes(take_array::<16>(bytes, &mut cursor)?),
                    }
                }
                10 if candidate_v2 => Capability::BalanceView {
                    account: take_array::<32>(bytes, &mut cursor)?,
                    asset: take_array::<32>(bytes, &mut cursor)?,
                    receipt_digest: take_array::<32>(bytes, &mut cursor)?,
                },
                _ => return Err(AbiError::InvalidEncoding),
            };
            grants.push(grant);
        }
        if cursor != bytes.len() {
            return Err(AbiError::InvalidEncoding);
        }
        let canonical = Self::new(grants.clone())?.canonical_encoding();
        if canonical != bytes {
            return Err(AbiError::InvalidEncoding);
        }
        Ok(grants)
    }

    pub(super) fn receipt_digests(&self) -> impl Iterator<Item = [u8; 32]> + '_ {
        self.0.values().filter_map(|capability| match capability {
            Capability::ReceiptRead { receipt_digest } => Some(*receipt_digest),
            _ => None,
        })
    }

    pub(super) fn balance_grants(
        &self,
    ) -> impl Iterator<Item = ([u8; 32], [u8; 32], [u8; 32])> + '_ {
        self.0.values().filter_map(|capability| match capability {
            Capability::BalanceView {
                account,
                asset,
                receipt_digest,
            } => Some((*account, *asset, *receipt_digest)),
            _ => None,
        })
    }
}

fn take_array<const N: usize>(bytes: &[u8], cursor: &mut usize) -> Result<[u8; N], AbiError> {
    let end = cursor.checked_add(N).ok_or(AbiError::InvalidEncoding)?;
    let slice = bytes.get(*cursor..end).ok_or(AbiError::InvalidEncoding)?;
    let mut output = [0u8; N];
    output.copy_from_slice(slice);
    *cursor = end;
    Ok(output)
}

fn take_slice<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    length: usize,
) -> Result<&'a [u8], AbiError> {
    let end = cursor
        .checked_add(length)
        .ok_or(AbiError::InvalidEncoding)?;
    let slice = bytes.get(*cursor..end).ok_or(AbiError::InvalidEncoding)?;
    *cursor = end;
    Ok(slice)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{Capability, CapabilitySet, ProgramSpendAuthorization};
    use crate::abi::{
        Abi, AbiError, AuthorizationContext, CallFrameId, ReceiptOracle, ReceiptView,
    };
    use crate::accounts::{derive_program_account, MAX_PROGRAM_ACCOUNT_SEED_BYTES};
    use crate::execute::ABI_VERSION;
    use crate::storage::{PrincipalId, ProgramId, Storage};

    #[test]
    fn balance_sight_is_exact_and_confers_no_spending_authority() {
        let account = [41; 32];
        let asset = [42; 32];
        let receipt = [43; 32];
        let sight = Capability::BalanceView {
            account,
            asset,
            receipt_digest: receipt,
        };
        let grants = CapabilitySet::new([sight.clone()])
            .unwrap_or_else(|error| panic!("sight grant: {error}"));
        assert!(!grants.permits_transfer(asset, account, 1));
        assert!(!grants.has_program_spend());
        assert!(grants.narrow([sight]).is_ok());
        assert_eq!(
            grants.narrow([Capability::BalanceView {
                account,
                asset,
                receipt_digest: [44; 32],
            }]),
            Err(AbiError::CapabilityEscalation)
        );
    }

    #[test]
    fn balance_sight_count_and_candidate_encoding_are_bounded() {
        assert_eq!(super::super::MAX_BALANCE_VIEW_GRANTS, 32);
        let grants = (1_u8..=32).map(|index| {
            Capability::BalanceView {
                account: [index; 32],
                asset: [100; 32],
                receipt_digest: [101; 32],
            }
        });
        let bounded = CapabilitySet::new(grants)
            .unwrap_or_else(|error| panic!("bounded sight grants: {error}"));
        let encoded = bounded.canonical_encoding();
        assert!(CapabilitySet::decode_candidate_canonical(&encoded).is_ok());
        assert_eq!(
            CapabilitySet::decode_canonical(&encoded),
            Err(AbiError::InvalidEncoding)
        );
        let over_limit = (1_u8..=33).map(
            |index| Capability::BalanceView {
                account: [index; 32],
                asset: [102; 32],
                receipt_digest: [103; 32],
            },
        );
        assert_eq!(
            CapabilitySet::new(over_limit),
            Err(AbiError::InvalidCapability)
        );
    }
    struct NoReceipts;

    impl ReceiptOracle for NoReceipts {
        fn verified_receipt(&self, _: [u8; 32]) -> Result<ReceiptView, AbiError> {
            Err(AbiError::ReceiptMismatch)
        }
    }

    fn program(byte: u8) -> ProgramId {
        ProgramId::new([byte; 32]).unwrap_or_else(|error| panic!("program: {error}"))
    }

    fn principal(byte: u8) -> PrincipalId {
        PrincipalId::new([byte; 32]).unwrap_or_else(|error| panic!("principal: {error}"))
    }

    fn program_spend(
        owner_program: ProgramId,
        seed: &[u8],
        asset: [u8; 32],
        to: [u8; 32],
        maximum_amount: u128,
    ) -> Capability {
        let source_account = derive_program_account(owner_program, seed)
            .unwrap_or_else(|error| panic!("account: {error}"))
            .bytes();
        Capability::ProgramSpend {
            owner_program,
            seed: seed.to_vec(),
            source_account,
            asset,
            to,
            maximum_amount,
        }
    }

    #[test]
    fn frozen_v1_encoding_remains_exact_and_refuses_the_candidate_tag() {
        let frozen = CapabilitySet::new([Capability::StorageRead])
            .unwrap_or_else(|error| panic!("frozen grant: {error}"));
        assert_eq!(frozen.canonical_encoding(), [0, 1, 1]);
        assert_eq!(
            CapabilitySet::decode_canonical(&frozen.canonical_encoding()),
            Ok(vec![Capability::StorageRead])
        );

        let candidate = CapabilitySet::new([program_spend(
            program(1),
            b"vault/one",
            [2; 32],
            [3; 32],
            100,
        )])
        .unwrap_or_else(|error| panic!("candidate grant: {error}"));
        let encoded = candidate.canonical_encoding();
        assert_eq!(encoded[2], 9);
        assert_eq!(
            CapabilitySet::decode_canonical(&encoded),
            Err(AbiError::InvalidEncoding)
        );
        assert_eq!(
            CapabilitySet::decode_candidate_canonical(&encoded)
                .and_then(CapabilitySet::new)
                .map(|decoded| decoded.canonical_encoding()),
            Ok(encoded)
        );
    }

    #[test]
    fn candidate_decoder_rejects_unknown_noncanonical_and_unbound_grants() {
        assert_eq!(
            CapabilitySet::decode_candidate_canonical(&[0, 1, 0xff]),
            Err(AbiError::InvalidEncoding)
        );
        let owner = program(4);
        let mut wrong_source = derive_program_account(owner, b"actual")
            .unwrap_or_else(|error| panic!("account: {error}"))
            .bytes();
        wrong_source[0] ^= 1;
        assert_eq!(
            CapabilitySet::new([Capability::ProgramSpend {
                owner_program: owner,
                seed: b"actual".to_vec(),
                source_account: wrong_source,
                asset: [5; 32],
                to: [6; 32],
                maximum_amount: 1,
            }]),
            Err(AbiError::InvalidCapability)
        );
        assert_eq!(
            CapabilitySet::new([Capability::ProgramSpend {
                owner_program: owner,
                seed: vec![7; MAX_PROGRAM_ACCOUNT_SEED_BYTES + 1],
                source_account: [8; 32],
                asset: [5; 32],
                to: [6; 32],
                maximum_amount: 1,
            }]),
            Err(AbiError::InvalidCapability)
        );
    }

    #[test]
    fn program_spend_narrows_exact_identity_and_amount_across_every_edge() {
        let owner = program(10);
        let callee = program(11);
        let seed = b"escrow/order-7";
        let asset = [12; 32];
        let to = [13; 32];
        let originated = CapabilitySet::empty()
            .narrow_for_program_edge(owner, [program_spend(owner, seed, asset, to, 100)])
            .unwrap_or_else(|error| panic!("owner origin: {error}"));
        assert_eq!(
            CapabilitySet::empty()
                .narrow_for_program_edge(callee, [program_spend(owner, seed, asset, to, 100)]),
            Err(AbiError::CapabilityEscalation)
        );
        assert!(CapabilitySet::empty().contains_narrowed_for_program_edge(owner, &originated));
        assert!(!CapabilitySet::empty().contains_narrowed_for_program_edge(callee, &originated));
        let forwarded = originated
            .narrow_for_program_edge(callee, [program_spend(owner, seed, asset, to, 75)])
            .unwrap_or_else(|error| panic!("forward: {error}"));
        assert!(originated.contains_narrowed_for_program_edge(callee, &forwarded));

        let root = CapabilitySet::new([program_spend(owner, seed, asset, to, 100)])
            .unwrap_or_else(|error| panic!("root: {error}"));
        assert!(root.root_program_spend_is_owned_by(owner));
        assert!(!root.root_program_spend_is_owned_by(callee));
        let branch = root
            .narrow([program_spend(owner, seed, asset, to, 75)])
            .unwrap_or_else(|error| panic!("branch: {error}"));
        for visit in 1..=8 {
            let narrowed = branch.narrow([program_spend(owner, seed, asset, to, 75 - visit)]);
            assert!(narrowed.is_ok(), "visit {visit}");
        }
        for widened in [
            program_spend(owner, seed, asset, to, 101),
            program_spend(owner, seed, [14; 32], to, 75),
            program_spend(owner, seed, asset, [15; 32], 75),
            program_spend(owner, b"escrow/order-8", asset, to, 75),
            program_spend(program(16), seed, asset, to, 75),
        ] {
            assert_eq!(root.narrow([widened]), Err(AbiError::CapabilityEscalation));
        }
    }

    #[test]
    fn program_spend_handoff_requires_the_exact_owner_frame_and_aggregate_limit() {
        let owner = program(20);
        let callee = program(21);
        let seed = b"pool/base";
        let source = derive_program_account(owner, seed)
            .unwrap_or_else(|error| panic!("source: {error}"))
            .bytes();
        let asset = [22; 32];
        let to = [23; 32];
        let grants = CapabilitySet::new([program_spend(owner, seed, asset, to, 40)])
            .unwrap_or_else(|error| panic!("grant: {error}"));
        let authorization = |staging_program, owner_program, amount| ProgramSpendAuthorization {
            staging_program,
            owner_program,
            seed,
            source_account: source,
            asset,
            to,
            amount,
        };
        assert!(grants.permits_program_spend(authorization(owner, owner, 40)));
        assert!(!grants.permits_program_spend(authorization(owner, owner, 41)));
        assert!(!grants.permits_program_spend(authorization(callee, owner, 1)));
        assert!(!grants.permits_program_spend(authorization(owner, callee, 1)));
        assert!(!grants.permits_program_spend(authorization(owner, owner, 0)));
    }

    #[test]
    fn inherited_escalation_never_stages_a_descendant_edge_or_transfer() {
        let owner = program(30);
        let child = program(31);
        let grandchild = program(32);
        let actor = principal(33);
        let seed = b"escrow/atomic";
        let asset = [34; 32];
        let to = [35; 32];
        let root_grants = CapabilitySet::new([
            Capability::Call { program: child },
            Capability::Call {
                program: grandchild,
            },
        ])
        .unwrap_or_else(|error| panic!("root grants: {error}"));
        let mut root = Abi::new(
            ABI_VERSION,
            owner,
            AuthorizationContext::new(actor, root_grants),
            Storage::new(),
            &NoReceipts,
        )
        .unwrap_or_else(|error| panic!("root ABI: {error}"));
        let child_frame = CallFrameId::root()
            .child(1)
            .unwrap_or_else(|error| panic!("child frame: {error}"));
        let child_grants = root
            .stage_call(
                child,
                b"",
                vec![
                    Capability::Call {
                        program: grandchild,
                    },
                    program_spend(owner, seed, asset, to, 80),
                ],
                child_frame,
            )
            .unwrap_or_else(|error| panic!("owner grant: {error}"));
        assert_eq!(root.commit().effects.calls.len(), 1);

        let mut nested = Abi::nested(
            ABI_VERSION,
            child,
            AuthorizationContext::nested(actor, child_grants, child_frame),
            Storage::new(),
            BTreeMap::new(),
            BTreeMap::new(),
        )
        .unwrap_or_else(|error| panic!("nested ABI: {error}"));
        let grandchild_frame = child_frame
            .child(1)
            .unwrap_or_else(|error| panic!("grandchild frame: {error}"));
        for widened in [
            program_spend(owner, seed, asset, to, 81),
            program_spend(owner, seed, [36; 32], to, 80),
            program_spend(owner, seed, asset, [37; 32], 80),
        ] {
            assert_eq!(
                nested.stage_call(grandchild, b"", vec![widened], grandchild_frame),
                Err(AbiError::CapabilityEscalation)
            );
        }
        let effects = nested.commit().effects;
        assert!(effects.calls.is_empty());
        assert!(effects.transfers.is_empty());
    }
}
