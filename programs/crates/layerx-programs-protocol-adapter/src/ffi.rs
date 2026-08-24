use core::ffi::c_void;

use layerx_programs::{
    AccountStateError, AccountStateHead, AccountStateJournal, CanonicalAccountLeaf, ExitRoute,
    JournalAccountStateAuthority, LifecycleReceipt, ProgramId, ProgramLifecycle,
    ProgramValueAccountBinding, ProvenAccountLeaf, ProvenProgramBinding, ReadFreshness, Registry,
    RegistryError, StateProof, VerifiedAccountSnapshot, VerifiedProgramBalanceRead, WindDownPolicy,
    WindDownStateAccess,
};

const RESULT_OK: i32 = 0;
const RESULT_UNKNOWN_FIELD: i32 = -7;
const MAX_SEED_BYTES: usize = 128;
const MAX_ACCOUNT_NAME_BYTES: usize = 512;
const MAX_PROOF_DEPTH: usize = 32;
const PROTOCOL_VERSION_ACCOUNT_TREE: u16 = 2;
const RECORD_MAGIC: &[u8; 5] = b"LXPS1";
const MAX_RECORD_ITEMS: usize = 4_096;

#[repr(C)]
#[derive(Clone, Copy, Eq, PartialEq)]
struct CAmount {
    hi: u64,
    lo: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Eq, PartialEq)]
struct CProof {
    leaf_index: u32,
    leaf_count: u32,
    depth: u8,
    siblings: [[u8; 32]; MAX_PROOF_DEPTH],
}

#[repr(C)]
#[derive(Clone, Copy, Eq, PartialEq)]
struct CBinding {
    record_version: u8,
    program_id: [u8; 32],
    account_id: [u8; 32],
    asset_id: [u8; 32],
    seed_length: u16,
    seed: [u8; MAX_SEED_BYTES],
    registered_sequence: u64,
    registration_event_digest: [u8; 32],
}

#[repr(C)]
#[derive(Clone, Copy, Eq, PartialEq)]
struct CAccount {
    id: [u8; 32],
    name: [u8; MAX_ACCOUNT_NAME_BYTES],
    name_length: u16,
    kind: i32,
    balance: CAmount,
    asset_id: [u8; 32],
    has_asset: bool,
    next_sequence: u64,
    created_at_sequence: u64,
    frozen: bool,
    has_open_reference: bool,
    authority_key: [u8; 32],
    has_authority_key: bool,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CValueAccountView {
    binding: CBinding,
    account: CAccount,
    balance: CAmount,
    frozen: bool,
    observed_sequence: u64,
    observed_at: u64,
    receipt_digest: [u8; 32],
    account_root: [u8; 32],
    universal_root: [u8; 32],
    programs_root: [u8; 32],
    state_root: [u8; 32],
    account_proof: CProof,
    account_tree_proof: CProof,
    universal_root_proof: CProof,
    binding_proof: CProof,
    programs_root_proof: CProof,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CAccountStateHead {
    observed_sequence: u64,
    observed_at: u64,
    receipt_digest: [u8; 32],
    account_root: [u8; 32],
    universal_root: [u8; 32],
    programs_root: [u8; 32],
    state_root: [u8; 32],
    account_tree_proof: CProof,
    universal_root_proof: CProof,
    programs_root_proof: CProof,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CWindDownView {
    program_id: [u8; 32],
    status: i32,
    exit_program: [u8; 32],
    deadline: u64,
    effective_sequence: u64,
    value_account_count: u16,
    live_value_account_count: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CExitRoute {
    program_id: [u8; 32],
    account_id: [u8; 32],
    asset_id: [u8; 32],
    destination: [u8; 32],
    seed_length: u16,
    seed: [u8; MAX_SEED_BYTES],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CHistory {
    program_id: [u8; 32],
    prior: i32,
    current: i32,
    authority: [u8; 32],
    exit_program: [u8; 32],
    deadline: u64,
    effective_sequence: u64,
    value_account_count: u16,
    live_value_account_count: u16,
    account_root: [u8; 32],
}

type ValueVisitor = unsafe extern "C" fn(*const CValueAccountView, *mut c_void) -> i32;
type RouteVisitor = unsafe extern "C" fn(*const CExitRoute, *mut c_void) -> i32;
type HistoryVisitor = unsafe extern "C" fn(*const CHistory, *mut c_void) -> i32;

unsafe extern "C" {
    fn lxp_programs_value_account_iter(
        context: *mut c_void,
        program_id: *const u8,
        receipt_digest: *const u8,
        visitor: ValueVisitor,
        user: *mut c_void,
    ) -> i32;
    fn lxp_programs_account_state_head_read(
        context: *mut c_void,
        program_id: *const u8,
        receipt_digest: *const u8,
        head: *mut CAccountStateHead,
    ) -> i32;
    fn lxp_programs_wind_down_read(
        context: *mut c_void,
        program_id: *const u8,
        view: *mut CWindDownView,
    ) -> i32;
    fn lxp_programs_exit_route_iter(
        context: *mut c_void,
        program_id: *const u8,
        visitor: RouteVisitor,
        user: *mut c_void,
    ) -> i32;
    fn lxp_programs_wind_down_history_iter(
        context: *mut c_void,
        program_id: *const u8,
        visitor: HistoryVisitor,
        user: *mut c_void,
    ) -> i32;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolAdapterError {
    CoreRefused(i32),
    NonCanonicalView,
    AccountState(AccountStateError),
    Registry(RegistryError),
    CorruptRecord,
}

impl From<AccountStateError> for ProtocolAdapterError {
    fn from(value: AccountStateError) -> Self {
        Self::AccountState(value)
    }
}

impl From<RegistryError> for ProtocolAdapterError {
    fn from(value: RegistryError) -> Self {
        Self::Registry(value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolProgramStateRead {
    balances: VerifiedProgramBalanceRead,
    snapshot: VerifiedAccountSnapshot,
    bindings: Vec<ProgramValueAccountBinding>,
    lifecycle: ProgramLifecycle,
    routes: Vec<ExitRoute>,
    history: Vec<LifecycleReceipt>,
}

impl ProtocolProgramStateRead {
    #[must_use]
    pub const fn program(&self) -> ProgramId {
        self.balances.program()
    }

    #[must_use]
    pub const fn balances(&self) -> &VerifiedProgramBalanceRead {
        &self.balances
    }

    #[must_use]
    pub fn routes(&self) -> &[ExitRoute] {
        &self.routes
    }

    #[must_use]
    pub fn history(&self) -> &[LifecycleReceipt] {
        &self.history
    }

    #[must_use]
    pub fn into_balances(self) -> VerifiedProgramBalanceRead {
        self.balances
    }

    #[must_use]
    pub fn canonical_encode(&self) -> Result<Vec<u8>, ProtocolAdapterError> {
        let mut encoded = RECORD_MAGIC.to_vec();
        encoded.extend_from_slice(&self.balances.program().bytes());
        encoded.push(lifecycle_byte(self.lifecycle));
        put_bindings(&mut encoded, &self.bindings)?;
        put_routes(&mut encoded, &self.routes)?;
        put_history(&mut encoded, &self.history)?;
        put_snapshot(&mut encoded, &self.snapshot)?;
        Ok(encoded)
    }

    /// Restores one proof record only after an independent production node
    /// receipt resolver has supplied both its verified receipt facts and the
    /// current canonical account-state head. Stored bytes never authorize
    /// themselves and a historical-but-valid receipt is not published as a
    /// live balance read.
    pub fn restore_verified(
        bytes: &[u8],
        registry: &mut Registry,
        verified_receipt: AccountStateHead,
        current_head: AccountStateHead,
        now: u64,
        staleness_limit: u64,
    ) -> Result<Self, ProtocolAdapterError> {
        if now == 0 || staleness_limit == 0 {
            return Err(ProtocolAdapterError::CorruptRecord);
        }
        Self::restore_inner(
            bytes,
            registry,
            verified_receipt,
            current_head,
            now,
            staleness_limit,
        )
    }

    fn restore_inner(
        bytes: &[u8],
        registry: &mut Registry,
        verified_receipt: AccountStateHead,
        current_head: AccountStateHead,
        now: u64,
        staleness_limit: u64,
    ) -> Result<Self, ProtocolAdapterError> {
        let mut cursor = Cursor::new(bytes);
        if cursor.take(RECORD_MAGIC.len())? != RECORD_MAGIC {
            return Err(ProtocolAdapterError::CorruptRecord);
        }
        let program =
            ProgramId::new(cursor.array()?).map_err(|_| ProtocolAdapterError::CorruptRecord)?;
        let lifecycle = lifecycle(i32::from(cursor.byte()?))?;
        let bindings = take_bindings(&mut cursor, program)?;
        let routes = take_routes(&mut cursor)?;
        let history = take_history(&mut cursor, program)?;
        let snapshot = take_snapshot(&mut cursor)?;
        if !cursor.is_empty() {
            return Err(ProtocolAdapterError::CorruptRecord);
        }
        let snapshot_head = AccountStateHead {
            receipt_digest: snapshot.receipt_digest,
            state_root: snapshot.state_root,
            freshness: snapshot.freshness,
        };
        if verified_receipt != snapshot_head {
            return Err(ProtocolAdapterError::AccountState(
                AccountStateError::UnverifiedReceipt,
            ));
        }
        if current_head != verified_receipt {
            return Err(ProtocolAdapterError::AccountState(
                AccountStateError::StaleRead,
            ));
        }
        registry.replay_protocol_state(program, &bindings, &routes, lifecycle, &history)?;
        let journal = ProtocolJournal { head: current_head };
        let authority = JournalAccountStateAuthority::new(&journal, now, staleness_limit)?;
        let balances = registry.read_value_accounts(program, &snapshot, &authority)?;
        Ok(Self {
            balances,
            snapshot,
            bindings,
            lifecycle,
            routes,
            history,
        })
    }
}

fn lifecycle_byte(value: ProgramLifecycle) -> u8 {
    match value {
        ProgramLifecycle::Active => 1,
        ProgramLifecycle::Deprecated => 2,
        ProgramLifecycle::Tombstoned => 3,
    }
}

fn put_u16(encoded: &mut Vec<u8>, value: usize) -> Result<(), ProtocolAdapterError> {
    let value = u16::try_from(value).map_err(|_| ProtocolAdapterError::CorruptRecord)?;
    encoded.extend_from_slice(&value.to_be_bytes());
    Ok(())
}

fn put_binding(
    encoded: &mut Vec<u8>,
    value: &ProgramValueAccountBinding,
) -> Result<(), ProtocolAdapterError> {
    encoded.push(value.record_version);
    encoded.extend_from_slice(&value.program.bytes());
    encoded.extend_from_slice(&value.account_id);
    encoded.extend_from_slice(&value.asset_id);
    put_u16(encoded, value.seed.len())?;
    encoded.extend_from_slice(&value.seed);
    encoded.extend_from_slice(&value.registered_sequence.to_be_bytes());
    encoded.extend_from_slice(&value.registration_event_digest);
    Ok(())
}

fn put_bindings(
    encoded: &mut Vec<u8>,
    values: &[ProgramValueAccountBinding],
) -> Result<(), ProtocolAdapterError> {
    if values.len() > MAX_RECORD_ITEMS {
        return Err(ProtocolAdapterError::CorruptRecord);
    }
    put_u16(encoded, values.len())?;
    for value in values {
        put_binding(encoded, value)?;
    }
    Ok(())
}

fn put_routes(encoded: &mut Vec<u8>, values: &[ExitRoute]) -> Result<(), ProtocolAdapterError> {
    if values.len() > MAX_RECORD_ITEMS {
        return Err(ProtocolAdapterError::CorruptRecord);
    }
    put_u16(encoded, values.len())?;
    for value in values {
        encoded.extend_from_slice(&value.account_id);
        encoded.extend_from_slice(&value.asset_id);
        encoded.extend_from_slice(&value.destination);
        put_u16(encoded, value.seed.len())?;
        encoded.extend_from_slice(&value.seed);
    }
    Ok(())
}

fn put_history(
    encoded: &mut Vec<u8>,
    values: &[LifecycleReceipt],
) -> Result<(), ProtocolAdapterError> {
    if values.len() > MAX_RECORD_ITEMS {
        return Err(ProtocolAdapterError::CorruptRecord);
    }
    put_u16(encoded, values.len())?;
    for value in values {
        encoded.push(lifecycle_byte(value.prior));
        encoded.push(lifecycle_byte(value.current));
        encoded.extend_from_slice(&value.authority);
        encoded.extend_from_slice(&value.effective_sequence.to_be_bytes());
        encoded.extend_from_slice(&value.wind_down.exit_program);
        encoded.extend_from_slice(&value.wind_down.deadline.to_be_bytes());
        encoded.extend_from_slice(&value.live_value_accounts.to_be_bytes());
    }
    Ok(())
}

fn put_proof(encoded: &mut Vec<u8>, value: &StateProof) -> Result<(), ProtocolAdapterError> {
    encoded.extend_from_slice(&value.leaf_index.to_be_bytes());
    encoded.extend_from_slice(&value.leaf_count.to_be_bytes());
    if value.siblings.len() > MAX_PROOF_DEPTH {
        return Err(ProtocolAdapterError::CorruptRecord);
    }
    encoded
        .push(u8::try_from(value.siblings.len()).map_err(|_| ProtocolAdapterError::CorruptRecord)?);
    for sibling in &value.siblings {
        encoded.extend_from_slice(sibling);
    }
    Ok(())
}

fn put_snapshot(
    encoded: &mut Vec<u8>,
    value: &VerifiedAccountSnapshot,
) -> Result<(), ProtocolAdapterError> {
    if value.bindings.len() > MAX_RECORD_ITEMS || value.accounts.len() > MAX_RECORD_ITEMS {
        return Err(ProtocolAdapterError::CorruptRecord);
    }
    encoded.extend_from_slice(&value.protocol_version.to_be_bytes());
    encoded.extend_from_slice(&value.receipt_digest);
    encoded.extend_from_slice(&value.state_root);
    encoded.extend_from_slice(&value.universal_root);
    encoded.extend_from_slice(&value.programs_root);
    encoded.extend_from_slice(&value.account_root);
    encoded.extend_from_slice(&value.freshness.observed_sequence.to_be_bytes());
    encoded.extend_from_slice(&value.freshness.observed_at.to_be_bytes());
    put_proof(encoded, &value.account_tree_proof)?;
    put_proof(encoded, &value.universal_root_proof)?;
    put_proof(encoded, &value.programs_root_proof)?;
    put_u16(encoded, value.bindings.len())?;
    for binding in &value.bindings {
        put_binding(encoded, &binding.binding)?;
        put_proof(encoded, &binding.proof)?;
    }
    put_u16(encoded, value.accounts.len())?;
    for account in &value.accounts {
        encoded.extend_from_slice(&account.leaf.account_id);
        put_u16(encoded, account.leaf.name.len())?;
        encoded.extend_from_slice(&account.leaf.name);
        encoded.push(account.leaf.kind);
        encoded.extend_from_slice(&account.leaf.balance.to_be_bytes());
        encoded.extend_from_slice(&account.leaf.asset_id);
        encoded.push(u8::from(account.leaf.has_asset));
        encoded.extend_from_slice(&account.leaf.next_sequence.to_be_bytes());
        encoded.extend_from_slice(&account.leaf.created_at_sequence.to_be_bytes());
        encoded.push(u8::from(account.leaf.frozen));
        encoded.push(u8::from(account.leaf.has_open_reference));
        encoded.extend_from_slice(&account.leaf.authority_key);
        encoded.push(u8::from(account.leaf.has_authority_key));
        put_proof(encoded, &account.proof)?;
    }
    Ok(())
}

struct Cursor<'a> {
    remaining: &'a [u8],
}

impl<'a> Cursor<'a> {
    const fn new(remaining: &'a [u8]) -> Self {
        Self { remaining }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], ProtocolAdapterError> {
        let (value, remaining) = self
            .remaining
            .split_at_checked(length)
            .ok_or(ProtocolAdapterError::CorruptRecord)?;
        self.remaining = remaining;
        Ok(value)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], ProtocolAdapterError> {
        self.take(N)?
            .try_into()
            .map_err(|_| ProtocolAdapterError::CorruptRecord)
    }

    fn byte(&mut self) -> Result<u8, ProtocolAdapterError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, ProtocolAdapterError> {
        Ok(u16::from_be_bytes(self.array()?))
    }

    fn u32(&mut self) -> Result<u32, ProtocolAdapterError> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64, ProtocolAdapterError> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    const fn is_empty(&self) -> bool {
        self.remaining.is_empty()
    }
}

fn take_count(cursor: &mut Cursor<'_>) -> Result<usize, ProtocolAdapterError> {
    let count = usize::from(cursor.u16()?);
    if count > MAX_RECORD_ITEMS {
        return Err(ProtocolAdapterError::CorruptRecord);
    }
    Ok(count)
}

fn take_binding(
    cursor: &mut Cursor<'_>,
) -> Result<ProgramValueAccountBinding, ProtocolAdapterError> {
    let record_version = cursor.byte()?;
    let program =
        ProgramId::new(cursor.array()?).map_err(|_| ProtocolAdapterError::CorruptRecord)?;
    let account_id = cursor.array()?;
    let asset_id = cursor.array()?;
    let seed_length = usize::from(cursor.u16()?);
    if seed_length > MAX_SEED_BYTES {
        return Err(ProtocolAdapterError::CorruptRecord);
    }
    let seed = cursor.take(seed_length)?.to_vec();
    let registered_sequence = cursor.u64()?;
    let registration_event_digest = cursor.array()?;
    Ok(ProgramValueAccountBinding {
        record_version,
        program,
        account_id,
        seed,
        asset_id,
        registered_sequence,
        registration_event_digest,
    })
}

fn take_bindings(
    cursor: &mut Cursor<'_>,
    program: ProgramId,
) -> Result<Vec<ProgramValueAccountBinding>, ProtocolAdapterError> {
    let count = take_count(cursor)?;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let value = take_binding(cursor)?;
        if value.program != program {
            return Err(ProtocolAdapterError::CorruptRecord);
        }
        values.push(value);
    }
    Ok(values)
}

fn take_routes(cursor: &mut Cursor<'_>) -> Result<Vec<ExitRoute>, ProtocolAdapterError> {
    let count = take_count(cursor)?;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let account_id = cursor.array()?;
        let asset_id = cursor.array()?;
        let destination = cursor.array()?;
        let seed_length = usize::from(cursor.u16()?);
        if seed_length > MAX_SEED_BYTES {
            return Err(ProtocolAdapterError::CorruptRecord);
        }
        values.push(ExitRoute {
            seed: cursor.take(seed_length)?.to_vec(),
            account_id,
            asset_id,
            destination,
        });
    }
    Ok(values)
}

fn take_history(
    cursor: &mut Cursor<'_>,
    program: ProgramId,
) -> Result<Vec<LifecycleReceipt>, ProtocolAdapterError> {
    let count = take_count(cursor)?;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(LifecycleReceipt {
            program,
            prior: lifecycle(i32::from(cursor.byte()?))?,
            current: lifecycle(i32::from(cursor.byte()?))?,
            authority: cursor.array()?,
            effective_sequence: cursor.u64()?,
            wind_down: WindDownPolicy {
                exit_program: cursor.array()?,
                deadline: cursor.u64()?,
                state_access: WindDownStateAccess::ReadOnly,
            },
            live_value_accounts: cursor.u32()?,
        });
    }
    Ok(values)
}

fn take_proof(cursor: &mut Cursor<'_>) -> Result<StateProof, ProtocolAdapterError> {
    let leaf_index = cursor.u32()?;
    let leaf_count = cursor.u32()?;
    let depth = usize::from(cursor.byte()?);
    if depth > MAX_PROOF_DEPTH {
        return Err(ProtocolAdapterError::CorruptRecord);
    }
    let mut siblings = Vec::with_capacity(depth);
    for _ in 0..depth {
        siblings.push(cursor.array()?);
    }
    Ok(StateProof {
        leaf_index,
        leaf_count,
        siblings,
    })
}

fn take_snapshot(cursor: &mut Cursor<'_>) -> Result<VerifiedAccountSnapshot, ProtocolAdapterError> {
    let protocol_version = cursor.u16()?;
    let receipt_digest = cursor.array()?;
    let state_root = cursor.array()?;
    let universal_root = cursor.array()?;
    let programs_root = cursor.array()?;
    let account_root = cursor.array()?;
    let freshness = ReadFreshness {
        observed_sequence: cursor.u64()?,
        observed_at: cursor.u64()?,
    };
    let account_tree_proof = take_proof(cursor)?;
    let universal_root_proof = take_proof(cursor)?;
    let programs_root_proof = take_proof(cursor)?;
    let binding_count = take_count(cursor)?;
    let mut bindings = Vec::with_capacity(binding_count);
    for _ in 0..binding_count {
        bindings.push(ProvenProgramBinding {
            binding: take_binding(cursor)?,
            proof: take_proof(cursor)?,
        });
    }
    let account_count = take_count(cursor)?;
    let mut accounts = Vec::with_capacity(account_count);
    for _ in 0..account_count {
        let account_id = cursor.array()?;
        let name_length = usize::from(cursor.u16()?);
        if name_length > MAX_ACCOUNT_NAME_BYTES {
            return Err(ProtocolAdapterError::CorruptRecord);
        }
        accounts.push(ProvenAccountLeaf {
            leaf: CanonicalAccountLeaf {
                account_id,
                name: cursor.take(name_length)?.to_vec(),
                kind: cursor.byte()?,
                balance: u128::from_be_bytes(cursor.array()?),
                asset_id: cursor.array()?,
                has_asset: take_bool(cursor)?,
                next_sequence: cursor.u64()?,
                created_at_sequence: cursor.u64()?,
                frozen: take_bool(cursor)?,
                has_open_reference: take_bool(cursor)?,
                authority_key: cursor.array()?,
                has_authority_key: take_bool(cursor)?,
            },
            proof: take_proof(cursor)?,
        });
    }
    Ok(VerifiedAccountSnapshot {
        protocol_version,
        receipt_digest,
        state_root,
        universal_root,
        programs_root,
        account_root,
        account_tree_proof,
        universal_root_proof,
        programs_root_proof,
        freshness,
        bindings,
        accounts,
    })
}

fn take_bool(cursor: &mut Cursor<'_>) -> Result<bool, ProtocolAdapterError> {
    match cursor.byte()? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(ProtocolAdapterError::CorruptRecord),
    }
}

#[derive(Clone, Copy)]
struct ProtocolJournal {
    head: AccountStateHead,
}

impl AccountStateJournal for ProtocolJournal {
    fn account_state_head(
        &self,
        receipt_digest: [u8; 32],
    ) -> Result<AccountStateHead, AccountStateError> {
        if receipt_digest == self.head.receipt_digest {
            Ok(self.head)
        } else {
            Err(AccountStateError::UnverifiedReceipt)
        }
    }

    fn current_account_state_head(&self) -> Result<AccountStateHead, AccountStateError> {
        Ok(self.head)
    }
}

unsafe extern "C" fn collect_value(view: *const CValueAccountView, user: *mut c_void) -> i32 {
    if view.is_null() || user.is_null() {
        return -3;
    }
    let values = unsafe { &mut *user.cast::<Vec<CValueAccountView>>() };
    values.push(unsafe { *view });
    RESULT_OK
}

unsafe extern "C" fn collect_route(route: *const CExitRoute, user: *mut c_void) -> i32 {
    if route.is_null() || user.is_null() {
        return -3;
    }
    let routes = unsafe { &mut *user.cast::<Vec<CExitRoute>>() };
    routes.push(unsafe { *route });
    RESULT_OK
}

unsafe extern "C" fn collect_history(history: *const CHistory, user: *mut c_void) -> i32 {
    if history.is_null() || user.is_null() {
        return -3;
    }
    let records = unsafe { &mut *user.cast::<Vec<CHistory>>() };
    records.push(unsafe { *history });
    RESULT_OK
}

fn proof(value: CProof) -> Result<StateProof, ProtocolAdapterError> {
    let depth = usize::from(value.depth);
    if depth > MAX_PROOF_DEPTH {
        return Err(ProtocolAdapterError::NonCanonicalView);
    }
    Ok(StateProof {
        leaf_index: value.leaf_index,
        leaf_count: value.leaf_count,
        siblings: value.siblings[..depth].to_vec(),
    })
}

fn lifecycle(value: i32) -> Result<ProgramLifecycle, ProtocolAdapterError> {
    match value {
        1 => Ok(ProgramLifecycle::Active),
        2 => Ok(ProgramLifecycle::Deprecated),
        3 => Ok(ProgramLifecycle::Tombstoned),
        _ => Err(ProtocolAdapterError::NonCanonicalView),
    }
}

fn binding(value: CBinding) -> Result<ProgramValueAccountBinding, ProtocolAdapterError> {
    let length = usize::from(value.seed_length);
    if length > MAX_SEED_BYTES {
        return Err(ProtocolAdapterError::NonCanonicalView);
    }
    let program =
        ProgramId::new(value.program_id).map_err(|_| ProtocolAdapterError::NonCanonicalView)?;
    Ok(ProgramValueAccountBinding {
        record_version: value.record_version,
        program,
        account_id: value.account_id,
        seed: value.seed[..length].to_vec(),
        asset_id: value.asset_id,
        registered_sequence: value.registered_sequence,
        registration_event_digest: value.registration_event_digest,
    })
}

fn account(value: CAccount) -> Result<CanonicalAccountLeaf, ProtocolAdapterError> {
    let name_length = usize::from(value.name_length);
    if name_length > MAX_ACCOUNT_NAME_BYTES || value.kind < 0 || value.kind > i32::from(u8::MAX) {
        return Err(ProtocolAdapterError::NonCanonicalView);
    }
    Ok(CanonicalAccountLeaf {
        account_id: value.id,
        name: value.name[..name_length].to_vec(),
        kind: u8::try_from(value.kind).map_err(|_| ProtocolAdapterError::NonCanonicalView)?,
        balance: (u128::from(value.balance.hi) << 64) | u128::from(value.balance.lo),
        asset_id: value.asset_id,
        has_asset: value.has_asset,
        next_sequence: value.next_sequence,
        created_at_sequence: value.created_at_sequence,
        frozen: value.frozen,
        has_open_reference: value.has_open_reference,
        authority_key: value.authority_key,
        has_authority_key: value.has_authority_key,
    })
}

/// Reads one complete program directly from the committed Programs module and
/// account tree. The C iterator is the sole producer: it verifies the named
/// receipt at the current state head and returns exact primary/account/outer
/// proofs before Rust constructs any public balance value.
///
/// # Safety
///
/// `context` must be a live read-only `lxp_module_ctx` whose lifetime covers
/// this synchronous call. It must be bound to the canonical kernel account
/// registry and verified-receipt index as required by the C iterator.
pub unsafe fn read_program_state(
    context: *mut c_void,
    registry: &mut Registry,
    program: ProgramId,
    receipt_digest: [u8; 32],
    now: u64,
    staleness_limit: u64,
) -> Result<ProtocolProgramStateRead, ProtocolAdapterError> {
    if context.is_null() || receipt_digest == [0; 32] || now == 0 || staleness_limit == 0 {
        return Err(ProtocolAdapterError::NonCanonicalView);
    }
    let mut head = CAccountStateHead {
        observed_sequence: 0,
        observed_at: 0,
        receipt_digest: [0; 32],
        account_root: [0; 32],
        universal_root: [0; 32],
        programs_root: [0; 32],
        state_root: [0; 32],
        account_tree_proof: CProof {
            leaf_index: 0,
            leaf_count: 0,
            depth: 0,
            siblings: [[0; 32]; MAX_PROOF_DEPTH],
        },
        universal_root_proof: CProof {
            leaf_index: 0,
            leaf_count: 0,
            depth: 0,
            siblings: [[0; 32]; MAX_PROOF_DEPTH],
        },
        programs_root_proof: CProof {
            leaf_index: 0,
            leaf_count: 0,
            depth: 0,
            siblings: [[0; 32]; MAX_PROOF_DEPTH],
        },
    };
    let status = unsafe {
        lxp_programs_account_state_head_read(
            context,
            program.bytes().as_ptr(),
            receipt_digest.as_ptr(),
            &mut head,
        )
    };
    if status != RESULT_OK {
        return Err(ProtocolAdapterError::CoreRefused(status));
    }
    if head.receipt_digest != receipt_digest
        || head.observed_sequence == 0
        || head.observed_at == 0
        || head.state_root == [0; 32]
        || head.account_root == [0; 32]
        || head.universal_root == [0; 32]
        || head.programs_root == [0; 32]
    {
        return Err(ProtocolAdapterError::NonCanonicalView);
    }
    let mut values = Vec::<CValueAccountView>::new();
    let status = unsafe {
        lxp_programs_value_account_iter(
            context,
            program.bytes().as_ptr(),
            receipt_digest.as_ptr(),
            collect_value,
            (&mut values as *mut Vec<CValueAccountView>).cast(),
        )
    };
    if status != RESULT_OK {
        return Err(ProtocolAdapterError::CoreRefused(status));
    }
    let mut bindings = Vec::with_capacity(values.len());
    let mut proven_bindings = Vec::with_capacity(values.len());
    let mut proven_accounts = Vec::with_capacity(values.len());
    for value in values {
        if value.receipt_digest != head.receipt_digest
            || value.observed_sequence != head.observed_sequence
            || value.observed_at != head.observed_at
            || value.state_root != head.state_root
            || value.account_root != head.account_root
            || value.universal_root != head.universal_root
            || value.programs_root != head.programs_root
            || value.balance.hi != value.account.balance.hi
            || value.balance.lo != value.account.balance.lo
            || value.frozen != value.account.frozen
            || value.account_tree_proof != head.account_tree_proof
            || value.universal_root_proof != head.universal_root_proof
            || value.programs_root_proof != head.programs_root_proof
        {
            return Err(ProtocolAdapterError::NonCanonicalView);
        }
        let binding = binding(value.binding)?;
        let account = account(value.account)?;
        if binding.program != program
            || account.account_id != binding.account_id
            || account.asset_id != binding.asset_id
            || account.created_at_sequence != binding.registered_sequence
        {
            return Err(ProtocolAdapterError::NonCanonicalView);
        }
        bindings.push(binding.clone());
        proven_bindings.push(ProvenProgramBinding {
            binding,
            proof: proof(value.binding_proof)?,
        });
        proven_accounts.push(ProvenAccountLeaf {
            leaf: account,
            proof: proof(value.account_proof)?,
        });
    }
    proven_bindings.sort_by_key(|value| value.binding.primary_key());
    proven_accounts.sort_by_key(|value| value.leaf.account_id);
    let snapshot = VerifiedAccountSnapshot {
        protocol_version: PROTOCOL_VERSION_ACCOUNT_TREE,
        receipt_digest,
        state_root: head.state_root,
        universal_root: head.universal_root,
        programs_root: head.programs_root,
        account_root: head.account_root,
        account_tree_proof: proof(head.account_tree_proof)?,
        universal_root_proof: proof(head.universal_root_proof)?,
        programs_root_proof: proof(head.programs_root_proof)?,
        freshness: ReadFreshness {
            observed_sequence: head.observed_sequence,
            observed_at: head.observed_at,
        },
        bindings: proven_bindings,
        accounts: proven_accounts,
    };
    let mut status_view = CWindDownView {
        program_id: [0; 32],
        status: 0,
        exit_program: [0; 32],
        deadline: 0,
        effective_sequence: 0,
        value_account_count: 0,
        live_value_account_count: 0,
    };
    let status =
        unsafe { lxp_programs_wind_down_read(context, program.bytes().as_ptr(), &mut status_view) };
    let live_count = proven_accounts
        .iter()
        .filter(|account| account.leaf.balance != 0)
        .count();
    let program_lifecycle = if status == RESULT_UNKNOWN_FIELD {
        ProgramLifecycle::Active
    } else if status == RESULT_OK
        && status_view.program_id == program.bytes()
        && status_view.exit_program == program.bytes()
        && status_view.deadline != 0
        && status_view.effective_sequence != 0
        && usize::from(status_view.value_account_count) == bindings.len()
        && usize::from(status_view.live_value_account_count) == live_count
    {
        lifecycle(status_view.status)?
    } else if status == RESULT_OK {
        return Err(ProtocolAdapterError::NonCanonicalView);
    } else {
        return Err(ProtocolAdapterError::CoreRefused(status));
    };
    let mut c_routes = Vec::<CExitRoute>::new();
    let route_status = unsafe {
        lxp_programs_exit_route_iter(
            context,
            program.bytes().as_ptr(),
            collect_route,
            (&mut c_routes as *mut Vec<CExitRoute>).cast(),
        )
    };
    if route_status != RESULT_OK {
        return Err(ProtocolAdapterError::CoreRefused(route_status));
    }
    let mut routes = Vec::with_capacity(c_routes.len());
    for route in c_routes {
        let length = usize::from(route.seed_length);
        if length > MAX_SEED_BYTES || route.program_id != program.bytes() {
            return Err(ProtocolAdapterError::NonCanonicalView);
        }
        routes.push(ExitRoute {
            seed: route.seed[..length].to_vec(),
            account_id: route.account_id,
            asset_id: route.asset_id,
            destination: route.destination,
        });
    }
    let mut c_history = Vec::<CHistory>::new();
    let history_status = unsafe {
        lxp_programs_wind_down_history_iter(
            context,
            program.bytes().as_ptr(),
            collect_history,
            (&mut c_history as *mut Vec<CHistory>).cast(),
        )
    };
    if history_status != RESULT_OK {
        return Err(ProtocolAdapterError::CoreRefused(history_status));
    }
    let mut history = Vec::with_capacity(c_history.len());
    for record in c_history {
        if record.program_id != program.bytes()
            || record.exit_program != program.bytes()
            || usize::from(record.value_account_count) != bindings.len()
            || record.live_value_account_count > record.value_account_count
            || record.account_root == [0; 32]
        {
            return Err(ProtocolAdapterError::NonCanonicalView);
        }
        history.push(LifecycleReceipt {
            program,
            prior: lifecycle(record.prior)?,
            current: lifecycle(record.current)?,
            authority: record.authority,
            effective_sequence: record.effective_sequence,
            wind_down: WindDownPolicy {
                exit_program: record.exit_program,
                deadline: record.deadline,
                state_access: WindDownStateAccess::ReadOnly,
            },
            live_value_accounts: u32::from(record.live_value_account_count),
        });
    }
    registry.replay_protocol_state(program, &bindings, &routes, program_lifecycle, &history)?;
    let journal = ProtocolJournal {
        head: AccountStateHead {
            receipt_digest,
            state_root: head.state_root,
            freshness: snapshot.freshness,
        },
    };
    let authority = JournalAccountStateAuthority::new(&journal, now, staleness_limit)?;
    let balances = registry.read_value_accounts(program, &snapshot, &authority)?;
    Ok(ProtocolProgramStateRead {
        balances,
        snapshot,
        bindings,
        lifecycle: program_lifecycle,
        routes,
        history,
    })
}
