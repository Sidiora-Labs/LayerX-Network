use layerx_human_test_support as support;

mod live_paxeer {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../layerx-paxeer-client/tests/exit.rs"
    ));

    use layerx_human_service::journeys::{
        ExitBoundaryError, ExitWallet, ExitWalletOutcome, ExitWalletRequest,
    };

    pub(super) struct LiveExit {
        anvil: Anvil,
        topology: Topology,
        evidence: ExitEvidence,
        client: EmergencyExit,
    }

    impl LiveExit {
        pub(super) fn launch() -> Self {
            let anvil = Anvil::launch();
            let topology = Topology::deploy(&anvil);
            let evidence = topology.prepare(&anvil);
            let client = exit_client(&anvil, topology.exit);
            Self {
                anvil,
                topology,
                evidence,
                client,
            }
        }

        pub(super) fn evidence(&self) -> ExitEvidence {
            self.evidence.clone()
        }

        pub(super) fn client(&self) -> EmergencyExit {
            self.client.clone()
        }

        pub(super) fn declare_emergency(&self) {
            self.anvil.transact(
                EMERGENCY_COUNCIL,
                self.topology.exit,
                &static_call(DECLARE_EMERGENCY, &[]),
                0,
            );
        }

        pub(super) fn mine(&self) {
            self.anvil.mine();
        }

        pub(super) fn recipient_balance(&self) -> u128 {
            token_balance(&self.anvil, self.topology.token, RECIPIENT)
        }

        pub(super) fn wallet(&self, drop_first_acknowledgement: bool) -> LiveWallet<'_> {
            LiveWallet {
                anvil: &self.anvil,
                submitted: None,
                drop_first_acknowledgement,
                submissions: 0,
            }
        }
    }

    pub(super) fn assert_ordinary_core_unavailable() {
        prove_core_endpoint_is_unavailable();
    }

    pub(super) struct LiveWallet<'a> {
        anvil: &'a Anvil,
        submitted: Option<(ExitWalletRequest, TransactionHash)>,
        drop_first_acknowledgement: bool,
        submissions: u64,
    }

    impl LiveWallet<'_> {
        pub(super) const fn submissions(&self) -> u64 {
            self.submissions
        }
    }

    impl ExitWallet for LiveWallet<'_> {
        fn submit_or_resolve(
            &mut self,
            request: &ExitWalletRequest,
        ) -> Result<ExitWalletOutcome, ExitBoundaryError> {
            if let Some((original, transaction)) = &self.submitted {
                if original != request {
                    return Err(ExitBoundaryError::ContractViolation);
                }
                return Ok(ExitWalletOutcome::Submitted(*transaction));
            }
            let transaction =
                self.anvil
                    .send(GOVERNANCE, Some(request.contract), &request.calldata, 0);
            self.submitted = Some((request.clone(), transaction));
            self.submissions = self.submissions.saturating_add(1);
            if self.drop_first_acknowledgement {
                self.drop_first_acknowledgement = false;
                Err(ExitBoundaryError::Unavailable)
            } else {
                Ok(ExitWalletOutcome::Submitted(transaction))
            }
        }
    }
}

use std::fs;

use layerx_human_service::audit::{
    verify_export, AuditChain, AuditEvent, JourneyKind, JourneyState,
};
use layerx_human_service::journeys::{
    ExitBoundaryError, ExitJourney, ExitJourneyError, ExitPlan, ExitStage,
    IrreversibleExitConfirmation, EXIT_CONFIRMATION_PHRASE, EXIT_IRREVERSIBILITY_NOTICE,
    EXIT_NORMAL_OPERATION_MESSAGE, EXIT_SETTINGS_SURFACE, EXIT_TITLE, ORDINARY_WITHDRAWAL_PATH,
};
use layerx_human_service::notify::JourneyId;
use layerx_human_service::store::PrincipalStore;
use layerx_human_service::trace::TraceId;

use live_paxeer::LiveExit;
use support::{directory, install_and_open, principal, retention_uniform, tenancy};

const BALANCE: u128 = 300;

#[test]
fn typed_confirmation_and_guidance_are_exact() {
    assert!(IrreversibleExitConfirmation::parse("get my money out").is_err());
    assert!(IrreversibleExitConfirmation::parse("GET MY MONEY OUT ").is_err());
    let confirmation = IrreversibleExitConfirmation::parse(EXIT_CONFIRMATION_PHRASE)
        .unwrap_or_else(|error| panic!("confirmation: {error:?}"));
    assert_ne!(confirmation.digest(), [0; 32]);
    assert_eq!(
        layerx_human_service::journeys::ExitStatus::settings_surface(),
        EXIT_SETTINGS_SURFACE
    );
    assert_eq!(
        layerx_human_service::journeys::ExitStatus::title(),
        EXIT_TITLE
    );
    assert_eq!(
        layerx_human_service::journeys::ExitStatus::irreversibility_notice(),
        EXIT_IRREVERSIBILITY_NOTICE
    );
    assert_eq!(EXIT_NORMAL_OPERATION_MESSAGE, "Emergency exit is unavailable because the network is operating normally. Use ordinary withdrawal instead.");
    assert_eq!(ORDINARY_WITHDRAWAL_PATH, "/app/withdraw");
}

#[test]
#[allow(clippy::too_many_lines)]
fn degraded_core_ack_gap_and_restarts_converge_on_one_finalised_exit() {
    live_paxeer::assert_ordinary_core_unavailable();
    let live = LiveExit::launch();
    let client = live.client();
    let root = directory("exit-restart");
    let map = tenancy(&[("alice", "tenant-alpha"), ("bob", "tenant-beta")]);
    let retention = retention_uniform(50_000);
    let (mut store, digest) = install_and_open(&root, &map, retention);
    let owner = principal("alice");
    let trace = TraceId::mint([0x61; 16]);
    let normal_plan = plan("jrn_exitnormal0001", [0x31; 32], live.evidence());
    let confirmation = IrreversibleExitConfirmation::parse(EXIT_CONFIRMATION_PHRASE)
        .unwrap_or_else(|error| panic!("confirmation: {error:?}"));
    {
        let mut wallet = live.wallet(false);
        let mut scope = store
            .principal(&owner)
            .unwrap_or_else(|error| panic!("normal scope: {error}"));
        let mut audit =
            AuditChain::open(&scope).unwrap_or_else(|error| panic!("normal audit: {error}"));
        let mut journey = ExitJourney::start(
            &mut scope,
            &mut audit,
            &trace,
            &normal_plan,
            confirmation,
            100,
        )
        .unwrap_or_else(|error| panic!("normal start: {error}"));
        let status = journey
            .advance(&mut scope, &mut audit, &trace, &client, &mut wallet, 101)
            .unwrap_or_else(|error| panic!("normal advance: {error}"));
        assert_eq!(
            status.normal_operation_message(),
            Some(EXIT_NORMAL_OPERATION_MESSAGE)
        );
        assert_eq!(
            status.stage(),
            &ExitStage::UnavailableWhileNetworkOperatingNormally {
                ordinary_withdrawal_path: ORDINARY_WITHDRAWAL_PATH,
            }
        );
        assert_eq!(wallet.submissions(), 0);
        assert_eq!(live.recipient_balance(), 0);
    }

    live.declare_emergency();
    let mut wallet = live.wallet(true);
    let plan = plan("jrn_exitrestart0001", [0x41; 32], live.evidence());

    {
        let mut scope = store
            .principal(&owner)
            .unwrap_or_else(|error| panic!("scope: {error}"));
        let mut audit = AuditChain::open(&scope).unwrap_or_else(|error| panic!("audit: {error}"));
        let mut journey =
            ExitJourney::start(&mut scope, &mut audit, &trace, &plan, confirmation, 200)
                .unwrap_or_else(|error| panic!("start: {error}"));
        assert_eq!(
            journey
                .advance(&mut scope, &mut audit, &trace, &client, &mut wallet, 201,)
                .unwrap_or_else(|error| panic!("construct: {error}"))
                .stage(),
            &ExitStage::WaitingForWallet
        );
        assert!(matches!(
            journey.advance(&mut scope, &mut audit, &trace, &client, &mut wallet, 202,),
            Err(ExitJourneyError::Boundary(ExitBoundaryError::Unavailable))
        ));
        assert_eq!(
            journey
                .status()
                .unwrap_or_else(|error| panic!("status: {error}"))
                .stage(),
            &ExitStage::WaitingForWallet
        );
        assert_eq!(wallet.submissions(), 1);
    }

    drop(store);
    let mut store = PrincipalStore::open(&root, retention, digest)
        .unwrap_or_else(|error| panic!("restart store: {error}"));
    {
        let mut scope = store
            .principal(&owner)
            .unwrap_or_else(|error| panic!("restart scope: {error}"));
        let mut audit =
            AuditChain::open(&scope).unwrap_or_else(|error| panic!("restart audit: {error}"));
        let mut journey = ExitJourney::load(&scope, &plan.journey_id)
            .unwrap_or_else(|error| panic!("load: {error}"))
            .unwrap_or_else(|| panic!("exit missing after restart"));
        assert!(matches!(
            journey
                .advance(&mut scope, &mut audit, &trace, &client, &mut wallet, 203,)
                .unwrap_or_else(|error| panic!("resolve wallet action: {error}"))
                .stage(),
            ExitStage::ConfirmingPaxeer { .. }
        ));
        assert_eq!(wallet.submissions(), 1);
        let confirming = journey
            .advance(&mut scope, &mut audit, &trace, &client, &mut wallet, 204)
            .unwrap_or_else(|error| panic!("first finality poll: {error}"));
        assert!(matches!(
            confirming.stage(),
            ExitStage::ConfirmingPaxeer {
                confirmations: 1,
                required: 2,
                ..
            }
        ));
    }

    drop(store);
    live.mine();
    let mut store = PrincipalStore::open(&root, retention, digest)
        .unwrap_or_else(|error| panic!("second restart store: {error}"));
    let mut scope = store
        .principal(&owner)
        .unwrap_or_else(|error| panic!("second restart scope: {error}"));
    let mut audit =
        AuditChain::open(&scope).unwrap_or_else(|error| panic!("second audit: {error}"));
    let mut journey = ExitJourney::load(&scope, &plan.journey_id)
        .unwrap_or_else(|error| panic!("second load: {error}"))
        .unwrap_or_else(|| panic!("exit missing after second restart"));
    let status = journey
        .advance(&mut scope, &mut audit, &trace, &client, &mut wallet, 205)
        .unwrap_or_else(|error| panic!("finality: {error}"));
    let ExitStage::Done(finality) = status.stage() else {
        panic!("exit was not final: {:?}", status.stage());
    };
    assert_eq!(finality.confirmations, 2);
    assert_ne!(finality.block_hash, [0; 32]);
    assert_eq!(wallet.submissions(), 1);
    assert_eq!(live.recipient_balance(), BALANCE);

    let terminal = journey
        .advance(&mut scope, &mut audit, &trace, &client, &mut wallet, 206)
        .unwrap_or_else(|error| panic!("terminal replay: {error}"));
    assert_eq!(terminal, status);
    assert_eq!(wallet.submissions(), 1);
    assert_eq!(live.recipient_balance(), BALANCE);

    let entries = audit
        .entries(&scope)
        .unwrap_or_else(|error| panic!("audit entries: {error}"));
    assert!(entries.iter().any(|entry| matches!(
        entry.event(),
        AuditEvent::JourneyTransition {
            kind: JourneyKind::Exit,
            to: JourneyState::DoneFinalised,
            ..
        }
    ) && !entry.evidence().is_empty()));
    let bundle = audit
        .export(&scope)
        .unwrap_or_else(|error| panic!("audit export: {error}"));
    let report = verify_export(&bundle).unwrap_or_else(|error| panic!("verify export: {error}"));
    assert!(report.entries() >= 6);
    assert!(report.evidence_rows() >= 4);

    drop(scope);
    let bob = principal("bob");
    let bob_scope = store
        .principal(&bob)
        .unwrap_or_else(|error| panic!("bob scope: {error}"));
    assert!(
        ExitJourney::load(&bob_scope, &plan.journey_id)
            .unwrap_or_else(|error| panic!("bob load: {error}"))
            .is_none(),
        "the exit journey must remain owned by its principal"
    );
    drop(bob_scope);
    drop(store);
    fs::remove_dir_all(&root).unwrap_or_else(|error| panic!("cleanup: {error}"));
}

fn plan(
    journey: &str,
    idempotency_key: [u8; 32],
    evidence: layerx_paxeer_client::ExitEvidence,
) -> ExitPlan {
    ExitPlan {
        journey_id: JourneyId::new(journey).unwrap_or_else(|error| panic!("journey id: {error}")),
        idempotency_key,
        evidence,
    }
}
