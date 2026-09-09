use layerx_human_test_support as support;

use std::fs;

use layerx_human_service::approvals::InboxState;
use layerx_human_service::audit::AuditChain;
use layerx_human_service::journeys::JourneyState;
use layerx_human_service::notify::{
    ActiveShell, ActivityEntryId, AgentId, ApprovalId, Channel, DeepLinks, DegradedComponent,
    DeviceId, Dispatcher, Event, EventId, JourneyId, JourneyOutcome, LandingState, Money,
    NotificationGroup, NotificationId, NotificationSummary, Preferences, Recency, Resolution,
    SubjectState, Surface,
};
use layerx_human_service::store::{PrincipalStore, Table};
use layerx_human_service::trace::TraceId;
use support::{directory, install_and_open, principal, retention_uniform, row_key, tenancy};

const DAY: u64 = 86_400;

fn money(amount: u128) -> Money {
    Money::new(amount, "LXP").unwrap_or_else(|error| panic!("money: {error}"))
}

fn approval(value: &str) -> ApprovalId {
    ApprovalId::new(value).unwrap_or_else(|error| panic!("approval: {error}"))
}

fn agent(value: &str) -> AgentId {
    AgentId::new(value).unwrap_or_else(|error| panic!("agent: {error}"))
}

fn journey(value: &str) -> JourneyId {
    JourneyId::new(value).unwrap_or_else(|error| panic!("journey: {error}"))
}

fn activity(value: &str) -> ActivityEntryId {
    ActivityEntryId::new(value).unwrap_or_else(|error| panic!("activity: {error}"))
}

fn device(value: &str) -> DeviceId {
    DeviceId::new(value).unwrap_or_else(|error| panic!("device: {error}"))
}

fn occurrence(value: &str) -> EventId {
    EventId::new(value).unwrap_or_else(|error| panic!("event: {error}"))
}

#[test]
#[allow(clippy::too_many_lines)]
fn resolves_every_notification_to_both_shells_and_lands_on_current_subject_state() {
    let root = directory("notify-links-shells");
    let map = tenancy(&[("alice", "tenant-a")]);
    let (mut store, _digest) = install_and_open(&root, &map, retention_uniform(100));
    let mut scope = store
        .principal(&principal("alice"))
        .unwrap_or_else(|error| panic!("scope: {error}"));
    let mut audit = AuditChain::open(&scope).unwrap_or_else(|error| panic!("audit: {error}"));
    let trace = TraceId::mint([11; 16]);

    let approval_id = approval("apr_link");
    let approval_report = Dispatcher::dispatch(
        &mut scope,
        &mut audit,
        100,
        &trace,
        &Event::ApprovalWaiting {
            approval_id: approval_id.clone(),
            agent_id: agent("agt_link"),
            money: Some(money(25)),
        },
    )
    .unwrap_or_else(|error| panic!("approval notification: {error}"));
    let approval_delivery = approval_report
        .deliveries()
        .first()
        .unwrap_or_else(|| panic!("approval notification did not produce a channel delivery"));

    let completed_id = journey("jrn_completed");
    let completed_report = Dispatcher::dispatch(
        &mut scope,
        &mut audit,
        101,
        &trace,
        &Event::JourneyFinished {
            journey_id: completed_id.clone(),
            outcome: JourneyOutcome::Completed,
            money: Some(money(25)),
        },
    )
    .unwrap_or_else(|error| panic!("completed notification: {error}"));
    let completed_delivery = completed_report
        .deliveries()
        .first()
        .unwrap_or_else(|| panic!("completed notification did not produce a channel delivery"));

    let failed_id = journey("jrn_failedlink");
    let failed_report = Dispatcher::dispatch(
        &mut scope,
        &mut audit,
        102,
        &trace,
        &Event::JourneyFinished {
            journey_id: failed_id.clone(),
            outcome: JourneyOutcome::Failed,
            money: None,
        },
    )
    .unwrap_or_else(|error| panic!("failed notification: {error}"));
    let failed_delivery = failed_report
        .deliveries()
        .first()
        .unwrap_or_else(|| panic!("failed notification did not produce a channel delivery"));

    let claim_id = journey("jrn_claimlink");
    let claim_report = Dispatcher::dispatch(
        &mut scope,
        &mut audit,
        103,
        &trace,
        &Event::ClaimReady {
            journey_id: claim_id.clone(),
            money: money(25),
        },
    )
    .unwrap_or_else(|error| panic!("claim notification: {error}"));
    let claim_delivery = claim_report
        .deliveries()
        .first()
        .unwrap_or_else(|| panic!("claim notification did not produce a channel delivery"));

    let current_events = [
        Event::MoneyArrived {
            entry_id: activity("act_link"),
            journey_id: journey("jrn_arrived"),
            money: money(25),
        },
        Event::SecurityNewDevice {
            device_id: device("dev_link"),
        },
        Event::SecurityRecovery {
            event_id: occurrence("evt_linkrecovery"),
        },
        Event::SecurityWalletRebinding {
            event_id: occurrence("evt_linkwallet"),
        },
        Event::SecurityKeyRotation {
            event_id: occurrence("evt_linkkeys"),
        },
        Event::ServiceStatus {
            event_id: occurrence("evt_linkstatus"),
            component: DegradedComponent::AgentLayer,
        },
    ];
    let current_reports = current_events
        .iter()
        .enumerate()
        .map(|(offset, event)| {
            Dispatcher::dispatch(
                &mut scope,
                &mut audit,
                104 + u64::try_from(offset).unwrap_or_else(|_| panic!("offset overflow")),
                &trace,
                event,
            )
            .unwrap_or_else(|error| panic!("current notification: {error}"))
        })
        .collect::<Vec<_>>();

    for shell in [ActiveShell::Mobile, ActiveShell::Desktop] {
        let awaiting = InboxState::AwaitingApproval;
        let approval_landing = DeepLinks::resolve(
            approval_delivery,
            shell,
            SubjectState::Approval {
                approval_id: &approval_id,
                state: &awaiting,
            },
        )
        .unwrap_or_else(|error| panic!("approval landing: {error}"));
        assert_eq!(approval_landing.shell(), shell);
        assert_eq!(approval_landing.surface(), Surface::Approval);
        assert_eq!(approval_landing.state(), LandingState::Actionable);
        assert_eq!(approval_landing.path(), "/app/approvals/apr_link");

        let expired = InboxState::Expired;
        let stale_approval = DeepLinks::resolve(
            approval_delivery,
            shell,
            SubjectState::Approval {
                approval_id: &approval_id,
                state: &expired,
            },
        )
        .unwrap_or_else(|error| panic!("expired landing: {error}"));
        assert_eq!(
            stale_approval.state(),
            LandingState::Resolved(Resolution::Expired)
        );
        assert_eq!(stale_approval.surface(), Surface::Approval);

        let completed = DeepLinks::resolve(
            completed_delivery,
            shell,
            SubjectState::Journey {
                journey_id: &completed_id,
                state: JourneyState::Done,
            },
        )
        .unwrap_or_else(|error| panic!("completed landing: {error}"));
        assert_eq!(completed.surface(), Surface::Journey);
        assert_eq!(completed.path(), "/app/journeys/jrn_completed");
        assert_eq!(completed.state(), LandingState::Resolved(Resolution::Done));

        let failed = DeepLinks::resolve(
            failed_delivery,
            shell,
            SubjectState::Journey {
                journey_id: &failed_id,
                state: JourneyState::Refused,
            },
        )
        .unwrap_or_else(|error| panic!("failed landing: {error}"));
        assert_eq!(failed.state(), LandingState::Resolved(Resolution::Failed));

        let claim = DeepLinks::resolve(
            claim_delivery,
            shell,
            SubjectState::Journey {
                journey_id: &claim_id,
                state: JourneyState::Processing,
            },
        )
        .unwrap_or_else(|error| panic!("claim landing: {error}"));
        assert_eq!(claim.surface(), Surface::Claim);
        assert_eq!(claim.path(), "/app/journeys/jrn_claimlink/claim");
        assert_eq!(claim.state(), LandingState::Actionable);

        let claimed = DeepLinks::resolve(
            claim_delivery,
            shell,
            SubjectState::Journey {
                journey_id: &claim_id,
                state: JourneyState::Done,
            },
        )
        .unwrap_or_else(|error| panic!("claimed landing: {error}"));
        assert_eq!(claimed.surface(), Surface::Journey);
        assert_eq!(claimed.path(), "/app/journeys/jrn_claimlink");
        assert_eq!(claimed.state(), LandingState::Resolved(Resolution::Done));

        for report in &current_reports {
            let delivery = report
                .deliveries()
                .first()
                .unwrap_or_else(|| panic!("current event did not produce a channel delivery"));
            let landing = DeepLinks::resolve(delivery, shell, SubjectState::Current)
                .unwrap_or_else(|error| panic!("current landing: {error}"));
            assert_eq!(landing.shell(), shell);
            assert!(landing.path().starts_with("/app/"));
        }
    }

    let wrong_approval = approval("apr_other");
    assert!(DeepLinks::resolve(
        approval_delivery,
        ActiveShell::Desktop,
        SubjectState::Approval {
            approval_id: &wrong_approval,
            state: &InboxState::Expired,
        },
    )
    .is_err());
    assert!(DeepLinks::resolve(
        completed_delivery,
        ActiveShell::Mobile,
        SubjectState::Journey {
            journey_id: &completed_id,
            state: JourneyState::StillChecking,
        },
    )
    .is_err());

    fs::remove_dir_all(root).unwrap_or_else(|error| panic!("cleanup: {error}"));
}

#[test]
#[allow(clippy::too_many_lines)]
fn inventory_groups_by_recency_and_persists_read_state_without_duplicate_badges() {
    let root = directory("notify-links-inventory");
    let map = tenancy(&[("alice", "tenant-a")]);
    let (mut store, digest) = install_and_open(&root, &map, retention_uniform(100));
    let now = 20 * DAY + 10_000;
    let trace = TraceId::mint([12; 16]);
    let mut notification_ids = Vec::<NotificationId>::new();

    {
        let mut scope = store
            .principal(&principal("alice"))
            .unwrap_or_else(|error| panic!("scope: {error}"));
        let mut preferences = Preferences::default();
        preferences.set_channel(Channel::Push, false);
        preferences.set_channel(Channel::Email, false);
        Dispatcher::update_preferences(&mut scope, now.saturating_sub(10), &preferences)
            .unwrap_or_else(|error| panic!("preferences: {error}"));
        let mut audit = AuditChain::open(&scope).unwrap_or_else(|error| panic!("audit: {error}"));
        let events = [
            (
                now.saturating_sub(10),
                Event::ApprovalWaiting {
                    approval_id: approval("apr_today"),
                    agent_id: agent("agt_today"),
                    money: Some(money(10)),
                },
            ),
            (
                now.saturating_sub(DAY),
                Event::MoneyArrived {
                    entry_id: activity("act_yesterday"),
                    journey_id: journey("jrn_yesterday"),
                    money: money(20),
                },
            ),
            (
                now.saturating_sub(3 * DAY),
                Event::SecurityNewDevice {
                    device_id: device("dev_week"),
                },
            ),
            (
                now.saturating_sub(8 * DAY),
                Event::ServiceStatus {
                    event_id: occurrence("evt_earlier"),
                    component: DegradedComponent::Paxeer,
                },
            ),
        ];
        for (created_at, event) in &events {
            let report = Dispatcher::dispatch(&mut scope, &mut audit, *created_at, &trace, event)
                .unwrap_or_else(|error| panic!("notification: {error}"));
            assert_eq!(report.deliveries().len(), 1);
            notification_ids.push(report.notification_id().clone());
        }
        let repeated = Dispatcher::dispatch(
            &mut scope,
            &mut audit,
            now,
            &trace,
            &Event::ApprovalWaiting {
                approval_id: approval("apr_today"),
                agent_id: agent("agt_today"),
                money: Some(money(999)),
            },
        )
        .unwrap_or_else(|error| panic!("repeated notification: {error}"));
        assert_eq!(repeated.notification_id(), &notification_ids[0]);

        let inventory =
            DeepLinks::inventory(&scope, now).unwrap_or_else(|error| panic!("inventory: {error}"));
        assert_eq!(inventory.unread_count(), 4);
        assert_eq!(inventory.groups().len(), 4);
        assert_eq!(inventory.groups()[0].recency(), Recency::Today);
        assert_eq!(inventory.groups()[1].recency(), Recency::Yesterday);
        assert_eq!(inventory.groups()[2].recency(), Recency::ThisWeek);
        assert_eq!(inventory.groups()[3].recency(), Recency::Earlier);
        assert!(inventory
            .groups()
            .iter()
            .all(|group| group.notifications().len() == 1));

        for id in &notification_ids {
            let read = DeepLinks::mark_read(&mut scope, now, id)
                .unwrap_or_else(|error| panic!("mark read: {error}"));
            assert!(read.read());
            assert!(scope
                .get(
                    Table::Notifications,
                    &row_key(&format!("notify_read_{}", id.as_str())),
                )
                .is_some());
        }
        let idempotent =
            DeepLinks::mark_read(&mut scope, now.saturating_add(1), &notification_ids[0])
                .unwrap_or_else(|error| panic!("idempotent read: {error}"));
        assert!(idempotent.read());
    }
    drop(store);

    let mut reopened = PrincipalStore::open(&root, retention_uniform(100), digest)
        .unwrap_or_else(|error| panic!("reopen: {error}"));
    let scope = reopened
        .principal(&principal("alice"))
        .unwrap_or_else(|error| panic!("reopened scope: {error}"));
    let inventory =
        DeepLinks::inventory(&scope, now).unwrap_or_else(|error| panic!("inventory: {error}"));
    assert_eq!(inventory.unread_count(), 0);
    assert!(inventory
        .groups()
        .iter()
        .flat_map(NotificationGroup::notifications)
        .all(NotificationSummary::read));
    for id in &notification_ids {
        assert!(inventory.notification(id).is_some());
    }

    fs::remove_dir_all(root).unwrap_or_else(|error| panic!("cleanup: {error}"));
}
