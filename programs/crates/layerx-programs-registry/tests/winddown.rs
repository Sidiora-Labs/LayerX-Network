use layerx_programs::{
    account_tree_commitment, program_account_registration_commitment, programs_root_commitment,
    state_leaf_commitment, state_node_commitment, universal_root_commitment, AccountStateError,
    AccountStateHead, AccountStateJournal, CanonicalAccountLeaf, Deprecation, DeprecationRefusal,
    DeprecationRequest, DeploymentRecord, ExitRoute, JournalAccountStateAuthority,
    LegacyDeprecationRequest, ProgramLifecycle, ProgramValueAccountBinding, ProvenAccountLeaf,
    ProvenProgramBinding, ReadFreshness, Registry, RegistryError, StateProof,
    VerifiedAccountSnapshot, WindDownPolicy, WindDownStateAccess, MAX_PROGRAM_VALUE_ACCOUNTS,
};
use layerx_programs_runtime::{
    derive_program_account, hash_bytes, HashAlgorithm, ProgramId, UpgradePolicy,
};

const ACCOUNT_STATE_VECTORS: &str =
    include_str!("../../../../tests/vectors/program_account_state_v2.vec");

fn vector(name: &str) -> &'static str {
    ACCOUNT_STATE_VECTORS
        .lines()
        .find_map(|line| {
            line.strip_prefix(name)
                .and_then(|value| value.strip_prefix('='))
        })
        .unwrap_or_else(|| panic!("missing vector: {name}"))
}

fn vector_bytes(name: &str) -> Vec<u8> {
    let encoded = vector(name).as_bytes();
    assert_eq!(encoded.len() % 2, 0);
    encoded
        .chunks_exact(2)
        .map(|pair| {
            let digit = |byte: u8| match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                _ => panic!("non-hex vector: {name}"),
            };
            digit(pair[0]) << 4 | digit(pair[1])
        })
        .collect()
}

fn vector_hash(name: &str) -> [u8; 32] {
    vector_bytes(name)
        .try_into()
        .unwrap_or_else(|_| panic!("vector is not a hash: {name}"))
}

fn vector_binding(prefix: &str) -> ProgramValueAccountBinding {
    let program = ProgramId::new(vector_hash(&format!("{prefix}_program_id")))
        .unwrap_or_else(|error| panic!("vector program id: {error}"));
    ProgramValueAccountBinding {
        record_version: 2,
        program,
        seed: vector_bytes(&format!("{prefix}_seed")),
        account_id: vector_hash(&format!("{prefix}_account_id")),
        asset_id: vector_hash(&format!("{prefix}_asset_id")),
        registered_sequence: vector(&format!("{prefix}_registered_sequence"))
            .parse()
            .unwrap_or_else(|error| panic!("vector sequence: {error}")),
        registration_event_digest: vector_hash(&format!("{prefix}_event_digest")),
    }
}

fn registration_event(binding: &ProgramValueAccountBinding) -> Vec<u8> {
    use sha2::{Digest as _, Sha256};

    let mut event = b"LXPA1".to_vec();
    event.extend_from_slice(&binding.program.bytes());
    event.extend_from_slice(&binding.account_id);
    event.extend_from_slice(&binding.asset_id);
    event.extend_from_slice(&(binding.seed.len() as u16).to_be_bytes());
    event.extend_from_slice(&Sha256::digest(&binding.seed));
    event.extend_from_slice(&binding.registered_sequence.to_be_bytes());
    event
}

#[derive(Clone)]
struct ReceiptAuthority {
    receipt: [u8; 32],
    state_root: [u8; 32],
    freshness: ReadFreshness,
}

impl ReceiptAuthority {
    fn head(&self) -> AccountStateHead {
        AccountStateHead {
            receipt_digest: self.receipt,
            state_root: self.state_root,
            freshness: self.freshness,
        }
    }

    fn verifier(&self) -> JournalAccountStateAuthority<Self> {
        JournalAccountStateAuthority::new(self.clone(), self.freshness.observed_at, 1_000)
            .unwrap_or_else(|error| panic!("journal authority: {error}"))
    }
}

impl AccountStateJournal for ReceiptAuthority {
    fn account_state_head(
        &self,
        receipt_digest: [u8; 32],
    ) -> Result<AccountStateHead, AccountStateError> {
        if receipt_digest == self.receipt {
            Ok(self.head())
        } else {
            Err(AccountStateError::UnverifiedReceipt)
        }
    }

    fn current_account_state_head(&self) -> Result<AccountStateHead, AccountStateError> {
        Ok(self.head())
    }
}

#[derive(Clone)]
struct ReceiptHistory(Vec<ReceiptAuthority>);

impl ReceiptHistory {
    fn verifier(&self) -> JournalAccountStateAuthority<Self> {
        let now = self
            .0
            .iter()
            .map(|authority| authority.freshness.observed_at)
            .max()
            .unwrap_or(1);
        JournalAccountStateAuthority::new(self.clone(), now, 1_000)
            .unwrap_or_else(|error| panic!("history authority: {error}"))
    }
}

impl AccountStateJournal for ReceiptHistory {
    fn account_state_head(
        &self,
        receipt_digest: [u8; 32],
    ) -> Result<AccountStateHead, AccountStateError> {
        self.0
            .iter()
            .find(|authority| authority.receipt == receipt_digest)
            .ok_or(AccountStateError::UnverifiedReceipt)?
            .account_state_head(receipt_digest)
    }

    fn current_account_state_head(&self) -> Result<AccountStateHead, AccountStateError> {
        self.0
            .iter()
            .max_by_key(|authority| authority.freshness.observed_sequence)
            .map(ReceiptAuthority::head)
            .ok_or(AccountStateError::JournalUnavailable)
    }
}

fn program(byte: u8) -> ProgramId {
    ProgramId::new([byte; 32]).unwrap_or_else(|error| panic!("program id: {error}"))
}

fn deployed_registry(id: ProgramId) -> Registry {
    replayed_registry(id, 2)
}

fn legacy_registry(id: ProgramId) -> Registry {
    replayed_registry(id, 1)
}

fn replayed_registry(id: ProgramId, abi_version: u16) -> Registry {
    let module = vec![0, 97, 115, 109, 1, 0, 0, 0];
    let code_hash = hash_bytes(HashAlgorithm::Sha256, &module)
        .unwrap_or_else(|error| panic!("program code hash: {error}"));
    let record = DeploymentRecord {
        program: id,
        version: 1,
        abi_version,
        upgrade_policy: UpgradePolicy::Authority([4; 32]),
        old_code_hash: None,
        new_code_hash: code_hash,
        sequence: 1,
        observed_at: 1,
        module,
        migration: None,
    };
    let mut registry = Registry::new();
    registry
        .replay_journal(&[record])
        .unwrap_or_else(|error| panic!("registry deployment replay: {error}"));
    registry
}

fn binding(id: ProgramId, seed: u8, asset: u8, sequence: u64) -> ProgramValueAccountBinding {
    let seed_bytes = vec![seed];
    let mut binding = ProgramValueAccountBinding {
        record_version: 2,
        program: id,
        account_id: derive_program_account(id, &seed_bytes)
            .unwrap_or_else(|error| panic!("account derivation: {error}"))
            .bytes(),
        seed: seed_bytes,
        asset_id: [asset; 32],
        registered_sequence: sequence,
        registration_event_digest: [0; 32],
    };
    binding.registration_event_digest = program_account_registration_commitment(&binding);
    binding
}

fn account_name(account_id: [u8; 32]) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut name = b"module:programs:value:".to_vec();
    for byte in account_id {
        name.push(HEX[usize::from(byte >> 4)]);
        name.push(HEX[usize::from(byte & 0x0f)]);
    }
    name
}

fn account(
    binding: &ProgramValueAccountBinding,
    balance: u128,
    frozen: bool,
) -> CanonicalAccountLeaf {
    CanonicalAccountLeaf {
        account_id: binding.account_id,
        name: account_name(binding.account_id),
        kind: 13,
        balance,
        asset_id: binding.asset_id,
        has_asset: true,
        next_sequence: 0,
        created_at_sequence: binding.registered_sequence,
        frozen,
        has_open_reference: false,
        authority_key: [0; 32],
        has_authority_key: false,
    }
}

fn tree(leaves: &[[u8; 32]]) -> ([u8; 32], Vec<StateProof>) {
    assert!(!leaves.is_empty());
    let mut proofs = Vec::new();
    for target in 0..leaves.len() {
        let mut level = leaves.to_vec();
        let mut index = target;
        let mut siblings = Vec::new();
        while level.len() > 1 {
            let sibling = if (index ^ 1) < level.len() {
                index ^ 1
            } else {
                index
            };
            siblings.push(level[sibling]);
            level = level
                .chunks(2)
                .map(|pair| state_node_commitment(pair[0], *pair.get(1).unwrap_or(&pair[0])))
                .collect();
            index /= 2;
        }
        proofs.push(StateProof {
            leaf_index: target as u32,
            leaf_count: leaves.len() as u32,
            siblings,
        });
    }
    let mut level = leaves.to_vec();
    while level.len() > 1 {
        level = level
            .chunks(2)
            .map(|pair| state_node_commitment(pair[0], *pair.get(1).unwrap_or(&pair[0])))
            .collect();
    }
    (level[0], proofs)
}

fn snapshot(
    bindings: &[ProgramValueAccountBinding],
    balances: &[u128],
    frozen: &[bool],
    sequence: u64,
) -> (VerifiedAccountSnapshot, ReceiptAuthority) {
    let mut leaves: Vec<CanonicalAccountLeaf> = bindings
        .iter()
        .zip(balances)
        .zip(frozen)
        .map(|((binding, balance), frozen)| account(binding, *balance, *frozen))
        .collect();
    leaves.sort_by_key(|leaf| leaf.account_id);
    let hashes: Vec<[u8; 32]> = leaves
        .iter()
        .map(|leaf| {
            leaf.commitment()
                .unwrap_or_else(|error| panic!("leaf: {error}"))
        })
        .collect();
    let (account_root, account_proofs) = tree(&hashes);

    let mut binding_records: Vec<ProgramValueAccountBinding> = bindings.to_vec();
    binding_records.sort_by_key(ProgramValueAccountBinding::primary_key);
    let binding_hashes: Vec<[u8; 32]> = binding_records
        .iter()
        .map(|binding| {
            binding
                .primary_commitment()
                .unwrap_or_else(|error| panic!("binding leaf: {error}"))
        })
        .collect();
    let (programs_root, binding_proofs) = tree(&binding_hashes);
    let proven_bindings = binding_records
        .into_iter()
        .zip(binding_proofs)
        .map(|(binding, proof)| ProvenProgramBinding { binding, proof })
        .collect();

    let account_tree_leaf = account_tree_commitment(account_root);
    let sequence_leaf = state_leaf_commitment(b"sequence", &(sequence + 1).to_be_bytes());
    let (universal_root, mut universal_proofs) = tree(&[account_tree_leaf, sequence_leaf]);

    let universal_leaf = universal_root_commitment(universal_root);
    let mut module_leaves = vec![universal_leaf];
    for module in 1_u16..9 {
        module_leaves.push(state_leaf_commitment(
            &module.to_be_bytes(),
            &[module as u8; 32],
        ));
    }
    module_leaves.push(programs_root_commitment(programs_root));
    let (state_root, state_proofs) = tree(&module_leaves);
    let receipt = [sequence as u8; 32];
    let proven = leaves
        .into_iter()
        .zip(account_proofs)
        .map(|(leaf, proof)| ProvenAccountLeaf { leaf, proof })
        .collect();
    (
        VerifiedAccountSnapshot {
            protocol_version: 2,
            receipt_digest: receipt,
            state_root,
            universal_root,
            programs_root,
            account_root,
            account_tree_proof: universal_proofs.remove(0),
            universal_root_proof: state_proofs[0].clone(),
            programs_root_proof: state_proofs[9].clone(),
            freshness: ReadFreshness {
                observed_sequence: sequence,
                observed_at: 1_700_000_000 + sequence,
            },
            bindings: proven_bindings,
            accounts: proven,
        },
        ReceiptAuthority {
            receipt,
            state_root,
            freshness: ReadFreshness {
                observed_sequence: sequence,
                observed_at: 1_700_000_000 + sequence,
            },
        },
    )
}

fn registry_with_accounts(id: ProgramId, bindings: &[ProgramValueAccountBinding]) -> Registry {
    let mut registry = deployed_registry(id);
    for binding in bindings {
        registry
            .record_value_account(binding.clone())
            .unwrap_or_else(|error| panic!("account registration: {error}"));
    }
    registry
}

fn routes(bindings: &[ProgramValueAccountBinding]) -> Vec<ExitRoute> {
    bindings
        .iter()
        .enumerate()
        .map(|(index, binding)| ExitRoute {
            seed: binding.seed.clone(),
            account_id: binding.account_id,
            asset_id: binding.asset_id,
            destination: [0x80 + index as u8; 32],
        })
        .collect()
}

fn request(
    id: ProgramId,
    exits: Vec<ExitRoute>,
    account_snapshot: VerifiedAccountSnapshot,
) -> DeprecationRequest {
    DeprecationRequest {
        program: id,
        expected: ProgramLifecycle::Active,
        target: ProgramLifecycle::Deprecated,
        authority: [4; 32],
        effective_sequence: account_snapshot.freshness.observed_sequence + 1,
        wind_down: WindDownPolicy {
            exit_program: id.bytes(),
            deadline: 500,
            state_access: WindDownStateAccess::ReadOnly,
        },
        exits,
        account_snapshot,
    }
}

#[test]
fn multiple_assets_and_accounts_resolve_only_from_the_canonical_tree() {
    let id = program(1);
    let bindings = vec![binding(id, 0x11, 0x21, 10), binding(id, 0x12, 0x22, 11)];
    let mut registry = registry_with_accounts(id, &bindings);
    let (account_snapshot, authority) = snapshot(&bindings, &[500, 900], &[false, false], 99);
    let mut deprecation = Deprecation::new();
    let transition = request(id, routes(&bindings), account_snapshot.clone());
    let receipt = deprecation
        .transition(&mut registry, &transition, &authority.verifier())
        .unwrap_or_else(|error| panic!("deprecation: {error}"));
    assert_eq!(receipt.live_value_accounts, 2);

    let view = deprecation
        .read(&registry, id, &account_snapshot, &authority.verifier())
        .unwrap_or_else(|error| panic!("wind-down read: {error}"));
    assert_eq!(view.status_label(), "deprecated");
    assert_eq!(view.reachable_value(), Some(1_400));
    assert_eq!(
        view.value_accounts[0].receipt_digest,
        account_snapshot.receipt_digest
    );
    assert_ne!(
        view.value_accounts[0].asset_id,
        view.value_accounts[1].asset_id
    );

    let exit = deprecation
        .authorize_exit(
            &registry,
            id,
            bindings[0].account_id,
            &account_snapshot,
            &authority.verifier(),
        )
        .unwrap_or_else(|error| panic!("authorized exit: {error}"));
    assert_eq!(exit.account.account_id, bindings[0].account_id);
    assert_eq!(exit.account.balance, 500);
    assert_eq!(exit.protocol_activity.activity_type, 0x0009_0007);
    assert_eq!(exit.protocol_activity.payload.len(), 65);
    assert_eq!(exit.protocol_activity.payload[32], 4);
    assert_eq!(
        &exit.protocol_activity.payload[33..],
        &bindings[0].account_id
    );

    assert_eq!(
        registry.record_value_account(binding(id, 0x13, 0x23, 100)),
        Err(AccountStateError::InactiveProgram)
    );
}

#[test]
fn altered_client_balance_is_refused_by_the_account_root() {
    let id = program(2);
    let bindings = vec![binding(id, 0x13, 0x23, 10)];
    let mut registry = registry_with_accounts(id, &bindings);
    let (mut account_snapshot, authority) = snapshot(&bindings, &[50], &[false], 99);
    account_snapshot.accounts[0].leaf.balance = 5_000;
    let transition = request(id, routes(&bindings), account_snapshot);
    assert_eq!(
        Deprecation::new().transition(&mut registry, &transition, &authority.verifier()),
        Err(DeprecationRefusal::AccountState(
            AccountStateError::AccountRootMismatch
        ))
    );
    assert_eq!(
        registry
            .entry_for_wind_down(id)
            .map(|entry| entry.lifecycle),
        Ok(ProgramLifecycle::Active)
    );
}

#[test]
fn missing_exit_or_frozen_value_refuses_without_partial_lifecycle_change() {
    let id = program(3);
    let bindings = vec![binding(id, 0x14, 0x24, 10), binding(id, 0x15, 0x25, 11)];
    let mut registry = registry_with_accounts(id, &bindings);
    let (account_snapshot, authority) = snapshot(&bindings, &[70, 80], &[false, false], 99);
    let transition = request(id, vec![routes(&bindings)[0].clone()], account_snapshot);
    assert_eq!(
        Deprecation::new().transition(&mut registry, &transition, &authority.verifier()),
        Err(DeprecationRefusal::MissingExit {
            account_id: bindings[1].account_id
        })
    );
    assert_eq!(
        registry.entry_for_wind_down(id).unwrap().lifecycle,
        ProgramLifecycle::Active
    );

    let (frozen_snapshot, frozen_authority) = snapshot(&bindings, &[70, 80], &[false, true], 99);
    let frozen = request(id, routes(&bindings), frozen_snapshot);
    assert_eq!(
        Deprecation::new().transition(&mut registry, &frozen, &frozen_authority.verifier(),),
        Err(DeprecationRefusal::ValueWouldBeStranded {
            account_id: bindings[1].account_id
        })
    );
    assert_eq!(
        registry.entry_for_wind_down(id).unwrap().lifecycle,
        ProgramLifecycle::Active
    );
}

#[test]
fn tombstone_replay_retains_history_state_and_exact_exit_path() {
    let id = program(4);
    let bindings = vec![binding(id, 0x16, 0x26, 10), binding(id, 0x17, 0x27, 11)];
    let base_registry = registry_with_accounts(id, &bindings);
    let (deprecate_snapshot, deprecate_authority) =
        snapshot(&bindings, &[100, 200], &[false, false], 99);
    let deprecate = request(id, routes(&bindings), deprecate_snapshot);
    let (tombstone_snapshot, tombstone_authority) =
        snapshot(&bindings, &[0, 200], &[false, false], 149);
    let mut tombstone = request(id, routes(&bindings), tombstone_snapshot.clone());
    tombstone.expected = ProgramLifecycle::Deprecated;
    tombstone.target = ProgramLifecycle::Tombstoned;
    tombstone.effective_sequence = 150;

    let history = ReceiptHistory(vec![deprecate_authority, tombstone_authority.clone()]);
    let mut registry = base_registry.clone();
    let mut state = Deprecation::new();
    let receipts = state
        .replay(&mut registry, &[tombstone, deprecate], &history.verifier())
        .unwrap_or_else(|error| panic!("replay: {error}"));
    assert_eq!(receipts.len(), 2);

    let view = state
        .read(
            &registry,
            id,
            &tombstone_snapshot,
            &tombstone_authority.verifier(),
        )
        .unwrap_or_else(|error| panic!("tombstone read: {error}"));
    assert_eq!(view.lifecycle, ProgramLifecycle::Tombstoned);
    assert_eq!(view.status_label(), "tombstoned");
    assert!(view.is_wound_down());
    assert_eq!(view.transition_history.len(), 2);
    assert_eq!(view.reachable_value(), Some(200));
    let exit = state
        .authorize_exit(
            &registry,
            id,
            bindings[1].account_id,
            &tombstone_snapshot,
            &tombstone_authority.verifier(),
        )
        .unwrap_or_else(|error| panic!("tombstone exit: {error}"));
    assert_eq!(exit.account.asset_id, bindings[1].asset_id);
    assert_eq!(exit.destination, [0x81; 32]);
}

#[test]
fn external_exit_program_is_not_mistaken_for_program_derived_authority() {
    let id = program(5);
    let bindings = vec![binding(id, 0x18, 0x28, 10)];
    let mut registry = registry_with_accounts(id, &bindings);
    let (account_snapshot, authority) = snapshot(&bindings, &[10], &[false], 99);
    let mut transition = request(id, routes(&bindings), account_snapshot);
    transition.wind_down.exit_program = [0xee; 32];
    assert_eq!(
        Deprecation::new().transition(&mut registry, &transition, &authority.verifier()),
        Err(DeprecationRefusal::InvalidExitProgram)
    );
}

#[test]
fn authority_deadline_and_duplicate_route_refusals_remain_typed_and_atomic() {
    let id = program(6);
    let bindings = vec![binding(id, 0x19, 0x29, 10), binding(id, 0x1a, 0x2a, 11)];
    let mut registry = registry_with_accounts(id, &bindings);
    let (account_snapshot, authority) = snapshot(&bindings, &[10, 20], &[false, false], 99);

    let mut wrong_authority = request(id, routes(&bindings), account_snapshot.clone());
    wrong_authority.authority = [0x99; 32];
    assert_eq!(
        Deprecation::new().transition(&mut registry, &wrong_authority, &authority.verifier(),),
        Err(DeprecationRefusal::Registry(
            RegistryError::InvalidLifecycleTransition
        ))
    );
    assert_eq!(
        registry.entry_for_wind_down(id).unwrap().lifecycle,
        ProgramLifecycle::Active
    );

    let mut expired = request(id, routes(&bindings), account_snapshot.clone());
    expired.wind_down.deadline = expired.effective_sequence;
    assert_eq!(
        Deprecation::new().transition(&mut registry, &expired, &authority.verifier()),
        Err(DeprecationRefusal::DeadlineElapsed)
    );

    let route = routes(&bindings)[0].clone();
    let duplicated = request(id, vec![route.clone(), route], account_snapshot);
    assert_eq!(
        Deprecation::new().transition(&mut registry, &duplicated, &authority.verifier()),
        Err(DeprecationRefusal::DuplicateExit)
    );
    assert_eq!(
        registry.entry_for_wind_down(id).unwrap().lifecycle,
        ProgramLifecycle::Active
    );
}

#[test]
fn funded_exit_remains_reachable_after_the_advisory_deadline() {
    let id = program(7);
    let bindings = vec![binding(id, 0x1e, 0x2e, 10)];
    let mut registry = registry_with_accounts(id, &bindings);
    let (transition_snapshot, transition_authority) = snapshot(&bindings, &[75], &[false], 99);
    let mut transition = request(id, routes(&bindings), transition_snapshot);
    transition.wind_down.deadline = 101;
    let mut deprecation = Deprecation::new();
    deprecation
        .transition(&mut registry, &transition, &transition_authority.verifier())
        .unwrap_or_else(|error| panic!("deprecation: {error}"));

    let (expired_snapshot, expired_authority) = snapshot(&bindings, &[75], &[false], 150);
    let exit = deprecation
        .authorize_exit(
            &registry,
            id,
            bindings[0].account_id,
            &expired_snapshot,
            &expired_authority.verifier(),
        )
        .unwrap_or_else(|error| panic!("post-deadline exit: {error}"));
    assert_eq!(exit.account.balance, 75);
    assert_eq!(exit.destination, [0x80; 32]);
}

#[test]
fn replay_reconstructs_identical_tombstone_history_and_live_balances() {
    let id = program(8);
    let bindings = vec![binding(id, 0x1b, 0x2b, 10), binding(id, 0x1c, 0x2c, 11)];
    let base_registry = registry_with_accounts(id, &bindings);
    let (deprecate_snapshot, deprecate_authority) =
        snapshot(&bindings, &[640, 20], &[false, false], 99);
    let deprecate_request = request(id, routes(&bindings), deprecate_snapshot);
    let (tombstone_snapshot, tombstone_authority) =
        snapshot(&bindings, &[640, 0], &[false, false], 149);
    let mut tombstone_request = request(id, routes(&bindings), tombstone_snapshot.clone());
    tombstone_request.expected = ProgramLifecycle::Deprecated;
    tombstone_request.target = ProgramLifecycle::Tombstoned;
    let authority = ReceiptHistory(vec![
        deprecate_authority.clone(),
        tombstone_authority.clone(),
    ]);

    let mut live_registry = base_registry.clone();
    let mut live = Deprecation::new();
    live.transition(
        &mut live_registry,
        &deprecate_request,
        &deprecate_authority.verifier(),
    )
    .unwrap_or_else(|error| panic!("live deprecation: {error}"));
    live.transition(
        &mut live_registry,
        &tombstone_request,
        &tombstone_authority.verifier(),
    )
    .unwrap_or_else(|error| panic!("live tombstone: {error}"));
    let live_view = live
        .read(
            &live_registry,
            id,
            &tombstone_snapshot,
            &tombstone_authority.verifier(),
        )
        .unwrap_or_else(|error| panic!("live view: {error}"));

    let mut replayed_registry = base_registry;
    let mut replayed = Deprecation::new();
    replayed
        .replay(
            &mut replayed_registry,
            &[tombstone_request, deprecate_request],
            &authority.verifier(),
        )
        .unwrap_or_else(|error| panic!("replay: {error}"));
    let replayed_view = replayed
        .read(
            &replayed_registry,
            id,
            &tombstone_snapshot,
            &tombstone_authority.verifier(),
        )
        .unwrap_or_else(|error| panic!("replayed view: {error}"));

    assert_eq!(replayed_view, live_view);
    assert_eq!(replayed_view.reachable_value(), Some(640));
}

#[test]
fn replay_refuses_a_logged_transition_that_would_strand_live_value() {
    let id = program(9);
    let bindings = vec![binding(id, 0x1d, 0x2d, 10)];
    let mut registry = registry_with_accounts(id, &bindings);
    let (account_snapshot, authority) = snapshot(&bindings, &[5], &[true], 99);
    let transition = request(id, routes(&bindings), account_snapshot);
    assert_eq!(
        Deprecation::new().replay(
            &mut registry,
            &[transition],
            &ReceiptHistory(vec![authority]).verifier(),
        ),
        Err(DeprecationRefusal::ValueWouldBeStranded {
            account_id: bindings[0].account_id
        })
    );
    assert_eq!(
        registry.entry_for_wind_down(id).unwrap().lifecycle,
        ProgramLifecycle::Active
    );
}

#[test]
fn frozen_abi_one_history_replays_only_with_an_empty_account_enumeration() {
    let id = program(10);
    let mut registry = legacy_registry(id);
    let mut deprecation = Deprecation::new();
    let policy = WindDownPolicy {
        exit_program: id.bytes(),
        deadline: 500,
        state_access: WindDownStateAccess::ReadOnly,
    };
    let first = LegacyDeprecationRequest {
        program: id,
        expected: ProgramLifecycle::Active,
        target: ProgramLifecycle::Deprecated,
        authority: [4; 32],
        effective_sequence: 100,
        wind_down: policy,
    };
    let second = LegacyDeprecationRequest {
        expected: ProgramLifecycle::Deprecated,
        target: ProgramLifecycle::Tombstoned,
        effective_sequence: 150,
        ..first
    };
    deprecation
        .transition_legacy(&mut registry, first)
        .unwrap_or_else(|error| panic!("legacy deprecation: {error}"));
    deprecation
        .transition_legacy(&mut registry, second)
        .unwrap_or_else(|error| panic!("legacy tombstone: {error}"));
    let view = deprecation
        .read_legacy(&registry, id)
        .unwrap_or_else(|error| panic!("legacy read: {error}"));
    assert_eq!(view.lifecycle, ProgramLifecycle::Tombstoned);
    assert_eq!(view.transition_history.len(), 2);
    assert!(view.value_accounts.is_empty());
}

#[test]
fn shared_c_rust_state_vectors_freeze_leaf_order_odd_duplication_and_bounds() {
    assert_eq!(vector("version"), "2");
    for prefix in ["registration", "empty", "maximum"] {
        let binding = vector_binding(prefix);
        assert_eq!(
            derive_program_account(binding.program, &binding.seed)
                .unwrap_or_else(|error| panic!("vector derivation: {error}"))
                .bytes(),
            binding.account_id
        );
        assert_eq!(
            registration_event(&binding),
            vector_bytes(&format!("{prefix}_event"))
        );
        assert_eq!(
            program_account_registration_commitment(&binding),
            binding.registration_event_digest
        );
        assert_eq!(
            binding.primary_key(),
            vector_bytes(&format!("{prefix}_primary_key"))
        );
        assert_eq!(
            binding.primary_value(),
            Ok(vector_bytes(&format!("{prefix}_primary_value")))
        );
        assert_eq!(
            binding.primary_commitment(),
            Ok(vector_hash(&format!("{prefix}_primary_commitment")))
        );
    }
    assert_eq!(vector("empty_seed"), "");
    assert_eq!(vector_bytes("maximum_seed").len(), 128);
    assert_eq!(vector("max_seed_length"), "128");
    assert_eq!(vector("refused_seed_length"), "129");
    assert!(derive_program_account(program(1), &[0; 129]).is_err());
    let order_a = vector_bytes("order_a_primary_key");
    let order_b = vector_bytes("order_b_primary_key");
    assert!(order_b < order_a);
    assert_eq!(
        vector_bytes("ordered_first_seed"),
        vector_bytes("order_b_seed")
    );
    assert_eq!(vector("max_program_accounts"), "512");
    assert_eq!(MAX_PROGRAM_VALUE_ACCOUNTS, 512);
    assert_eq!(vector("refused_program_accounts"), "513");
    let account_id = vector_hash("account_id");
    let asset_id = vector_hash("asset_id");
    let leaf = CanonicalAccountLeaf {
        account_id,
        name: vector_bytes("account_name"),
        kind: 13,
        balance: 0x12_3456,
        asset_id,
        has_asset: true,
        next_sequence: 7,
        created_at_sequence: 3,
        frozen: false,
        has_open_reference: true,
        authority_key: [0; 32],
        has_authority_key: false,
    };
    assert_eq!(leaf.commitment(), Ok(vector_hash("account_leaf")));
    let mut account_key = vec![4];
    account_key.extend_from_slice(&account_id);
    assert_eq!(
        state_leaf_commitment(&account_key, &vector_bytes("account_value")),
        vector_hash("account_leaf")
    );
    assert_eq!(state_leaf_commitment(&[0], &[0x10]), vector_hash("leaf0"));
    assert_eq!(state_leaf_commitment(&[1], &[0x20]), vector_hash("leaf1"));
    assert_eq!(state_leaf_commitment(&[2], &[0x30]), vector_hash("leaf2"));
    assert_eq!(
        state_node_commitment(vector_hash("leaf0"), vector_hash("leaf1")),
        vector_hash("node01")
    );
    let odd = state_node_commitment(vector_hash("leaf2"), vector_hash("proof2_sibling0"));
    assert_eq!(odd, vector_hash("node22"));
    assert_eq!(
        state_node_commitment(vector_hash("proof2_sibling1"), odd),
        vector_hash("tree_root")
    );
    assert_eq!(
        state_leaf_commitment(&0_u16.to_be_bytes(), &vector_hash("tree_root")),
        vector_hash("outer0_leaf")
    );
    assert_eq!(
        state_leaf_commitment(&9_u16.to_be_bytes(), &vector_hash("programs_root"),),
        vector_hash("outer9_leaf")
    );
    assert_eq!(
        state_node_commitment(vector_hash("outer0_leaf"), vector_hash("outer9_leaf"),),
        vector_hash("outer_root")
    );

    let id = program(11);
    let bindings = vec![binding(id, 0x1f, 0x2f, 10)];
    let (mut account_snapshot, authority) = snapshot(&bindings, &[1], &[false], 99);
    account_snapshot.accounts[0].proof.siblings = vec![[0; 32]; 33];
    assert_eq!(
        account_snapshot.verify(&authority.verifier()),
        Err(AccountStateError::ProofTooDeep)
    );
    assert_eq!(vector("max_proof_depth"), "32");
    assert_eq!(vector("refused_proof_depth"), "33");

    let (mut malformed, authority) = snapshot(&bindings, &[1], &[false], 100);
    malformed.accounts[0].proof.leaf_count = vector("malformed_proof_leaf_count")
        .parse()
        .unwrap_or_else(|error| panic!("vector leaf count: {error}"));
    malformed.accounts[0].proof.leaf_index = vector("malformed_proof_leaf_index")
        .parse()
        .unwrap_or_else(|error| panic!("vector leaf index: {error}"));
    assert_eq!(
        malformed.verify(&authority.verifier()),
        Err(AccountStateError::AccountRootMismatch)
    );
    malformed.accounts[0].proof.leaf_index = 0;
    malformed.accounts[0].proof.leaf_count = vector("zero_proof_leaf_count")
        .parse()
        .unwrap_or_else(|error| panic!("vector zero leaf count: {error}"));
    assert_eq!(
        malformed.verify(&authority.verifier()),
        Err(AccountStateError::AccountRootMismatch)
    );
}
