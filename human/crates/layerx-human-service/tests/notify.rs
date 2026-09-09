use layerx_human_test_support as support;

use std::fs;

use layerx_human_service::audit::{AuditChain, AuditEvent};
use layerx_human_service::notify::{
    ActivityEntryId, AgentId, ApprovalId, Channel, DegradedComponent, DetailLevel, DeviceId,
    DispatchOutcome, Dispatcher, Event, EventId, JourneyId, JourneyOutcome, Money,
    NotificationClass, Preferences,
};
use layerx_human_service::store::{PrincipalStore, Table};
use layerx_human_service::trace::TraceId;
use support::{directory, install_and_open, principal, retention_uniform, row_key, tenancy};

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

fn required_events() -> Vec<Event> {
    vec![
        Event::ApprovalWaiting {
            approval_id: approval("apr_one"),
            agent_id: agent("agt_one"),
            money: Some(money(25)),
        },
        Event::MoneyArrived {
            entry_id: activity("act_one"),
            journey_id: journey("jrn_deposit"),
            money: money(25),
        },
        Event::JourneyFinished {
            journey_id: journey("jrn_done"),
            outcome: JourneyOutcome::Completed,
            money: Some(money(25)),
        },
        Event::JourneyFinished {
            journey_id: journey("jrn_failed"),
            outcome: JourneyOutcome::Failed,
            money: Some(money(25)),
        },
        Event::ClaimReady {
            journey_id: journey("jrn_claim"),
            money: money(25),
        },
        Event::SecurityNewDevice {
            device_id: device("dev_one"),
        },
        Event::SecurityRecovery {
            event_id: occurrence("evt_recovery"),
        },
        Event::SecurityWalletRebinding {
            event_id: occurrence("evt_rebinding"),
        },
        Event::SecurityKeyRotation {
            event_id: occurrence("evt_rotation"),
        },
        Event::ServiceStatus {
            event_id: occurrence("evt_degraded"),
            component: DegradedComponent::AgentLayer,
        },
    ]
}

#[test]
fn dispatches_every_required_class_to_real_durable_channels_and_audits_each_effect() {
    let root = directory("notify-all-classes");
    let map = tenancy(&[("alice", "tenant-a")]);
    let (mut store, _digest) = install_and_open(&root, &map, retention_uniform(100));
    let mut scope = store
        .principal(&principal("alice"))
        .unwrap_or_else(|error| panic!("scope: {error}"));
    let mut audit = AuditChain::open(&scope).unwrap_or_else(|error| panic!("audit: {error}"));
    let trace = TraceId::mint([1; 16]);

    for (offset, event) in required_events().iter().enumerate() {
        let report = Dispatcher::dispatch(
            &mut scope,
            &mut audit,
            100 + u64::try_from(offset).unwrap_or_else(|_| panic!("offset overflow")),
            &trace,
            event,
        )
        .unwrap_or_else(|error| panic!("dispatch {:?}: {error}", event.class()));
        assert_eq!(report.outcome(), DispatchOutcome::Dispatched);
        assert_eq!(report.deliveries().len(), Channel::ALL.len());
        assert!(report
            .deliveries()
            .iter()
            .all(|delivery| delivery.deep_link().starts_with("/app/")));
        assert!(report
            .deliveries()
            .iter()
            .filter(|delivery| delivery.channel() == Channel::Push)
            .all(|delivery| delivery.payload().contains("lock_screen_copy_key")));
        assert!(report
            .deliveries()
            .iter()
            .filter(|delivery| delivery.channel() == Channel::Email)
            .all(|delivery| delivery.payload().contains("MIME-Version: 1.0")));
        assert!(report
            .deliveries()
            .iter()
            .filter(|delivery| delivery.channel() == Channel::InApp)
            .all(|delivery| delivery.payload().contains("\"read\":false")));
        assert!(report
            .deliveries()
            .iter()
            .all(|delivery| !delivery.payload().contains("25 LXP")));
        if event.class().security() || event.class() == NotificationClass::ApprovalWaiting {
            assert!(report
                .deliveries()
                .iter()
                .all(|delivery| delivery.action_copy_key().is_some()));
        }
    }

    let deliveries =
        Dispatcher::deliveries(&scope).unwrap_or_else(|error| panic!("list deliveries: {error}"));
    assert_eq!(
        deliveries.len(),
        required_events().len() * Channel::ALL.len()
    );
    let entries = audit
        .entries(&scope)
        .unwrap_or_else(|error| panic!("audit entries: {error}"));
    assert_eq!(entries.len(), deliveries.len());
    for entry in entries {
        assert!(matches!(
            entry.event(),
            AuditEvent::NotificationDispatch { .. }
        ));
        assert_eq!(entry.evidence().len(), 2);
        assert!(entry
            .evidence()
            .iter()
            .all(|evidence| evidence.table() == Table::Notifications));
    }
    fs::remove_dir_all(root).unwrap_or_else(|error| panic!("cleanup: {error}"));
}

#[test]
fn preference_changes_apply_immediately_and_critical_security_cannot_be_suppressed() {
    let root = directory("notify-preferences");
    let map = tenancy(&[("alice", "tenant-a")]);
    let (mut store, _digest) = install_and_open(&root, &map, retention_uniform(100));
    let mut scope = store
        .principal(&principal("alice"))
        .unwrap_or_else(|error| panic!("scope: {error}"));
    let mut audit = AuditChain::open(&scope).unwrap_or_else(|error| panic!("audit: {error}"));
    let trace = TraceId::mint([2; 16]);
    let mut preferences = Preferences::default();
    preferences.set_detail(DetailLevel::Full);
    preferences.set_channel(Channel::Email, false);
    preferences.set_channel(Channel::InApp, false);
    Dispatcher::update_preferences(&mut scope, 10, &preferences)
        .unwrap_or_else(|error| panic!("save preferences: {error}"));
    assert!(scope
        .get(Table::Notifications, &row_key("notify_preferences"))
        .is_some());

    let approval_report = Dispatcher::dispatch(
        &mut scope,
        &mut audit,
        11,
        &trace,
        &Event::ApprovalWaiting {
            approval_id: approval("apr_preference"),
            agent_id: agent("agt_preference"),
            money: Some(money(77)),
        },
    )
    .unwrap_or_else(|error| panic!("approval dispatch: {error}"));
    assert_eq!(approval_report.deliveries().len(), 1);
    assert_eq!(approval_report.deliveries()[0].channel(), Channel::Push);
    assert!(approval_report.deliveries()[0]
        .payload()
        .contains("\"amount\":\"77\""));

    preferences.set_detail(DetailLevel::Minimal);
    preferences.set_class(Channel::Push, NotificationClass::MoneyArrived, false);
    preferences.set_class(Channel::Push, NotificationClass::SecurityRecovery, false);
    preferences.set_class(
        Channel::Push,
        NotificationClass::SecurityWalletRebinding,
        false,
    );
    preferences.set_class(Channel::Push, NotificationClass::SecurityKeyRotation, false);
    Dispatcher::update_preferences(&mut scope, 12, &preferences)
        .unwrap_or_else(|error| panic!("replace preferences: {error}"));
    assert_eq!(
        Dispatcher::preferences(&scope)
            .unwrap_or_else(|error| panic!("reload preferences: {error}")),
        preferences
    );
    let suppressed = Dispatcher::dispatch(
        &mut scope,
        &mut audit,
        13,
        &trace,
        &Event::MoneyArrived {
            entry_id: activity("act_suppressed"),
            journey_id: journey("jrn_suppressed"),
            money: money(999),
        },
    )
    .unwrap_or_else(|error| panic!("suppressed dispatch: {error}"));
    assert_eq!(suppressed.outcome(), DispatchOutcome::Suppressed);
    assert!(suppressed.deliveries().is_empty());

    for event in [
        Event::SecurityRecovery {
            event_id: occurrence("evt_criticalrecovery"),
        },
        Event::SecurityWalletRebinding {
            event_id: occurrence("evt_criticalrebinding"),
        },
        Event::SecurityKeyRotation {
            event_id: occurrence("evt_criticalrotation"),
        },
    ] {
        let report = Dispatcher::dispatch(&mut scope, &mut audit, 14, &trace, &event)
            .unwrap_or_else(|error| panic!("critical dispatch: {error}"));
        assert_eq!(report.deliveries().len(), 1);
        let delivery = &report.deliveries()[0];
        assert_eq!(delivery.channel(), Channel::InApp);
        assert!(delivery.action_copy_key().is_some());
        assert!(!delivery.payload().contains("999"));
        assert!(delivery.payload().contains("notification.minimal"));
    }
    fs::remove_dir_all(root).unwrap_or_else(|error| panic!("cleanup: {error}"));
}

#[test]
fn repeated_events_and_restart_retries_converge_once_per_principal() {
    let root = directory("notify-dedup");
    let map = tenancy(&[("alice", "tenant-a"), ("bob", "tenant-b")]);
    let (mut store, digest) = install_and_open(&root, &map, retention_uniform(1));
    let trace = TraceId::mint([3; 16]);
    let event = Event::ClaimReady {
        journey_id: journey("jrn_dedup"),
        money: money(25),
    };
    {
        let mut scope = store
            .principal(&principal("alice"))
            .unwrap_or_else(|error| panic!("alice scope: {error}"));
        let mut preferences = Preferences::default();
        preferences.set_detail(DetailLevel::Full);
        Dispatcher::update_preferences(&mut scope, 20, &preferences)
            .unwrap_or_else(|error| panic!("save preferences: {error}"));
        let mut audit =
            AuditChain::open(&scope).unwrap_or_else(|error| panic!("alice audit: {error}"));
        let first = Dispatcher::dispatch(&mut scope, &mut audit, 21, &trace, &event)
            .unwrap_or_else(|error| panic!("first dispatch: {error}"));
        let changed_detail = Event::ClaimReady {
            journey_id: journey("jrn_dedup"),
            money: money(999),
        };
        let repeated = Dispatcher::dispatch(&mut scope, &mut audit, 22, &trace, &changed_detail)
            .unwrap_or_else(|error| panic!("repeat dispatch: {error}"));
        assert_eq!(repeated.outcome(), DispatchOutcome::Deduplicated);
        assert_eq!(repeated.deliveries(), first.deliveries());
        assert!(repeated.deliveries()[0]
            .payload()
            .contains("\"amount\":\"25\""));
        assert!(!repeated.deliveries()[0].payload().contains("999"));
        assert_eq!(
            audit
                .entries(&scope)
                .unwrap_or_else(|error| panic!("audit entries: {error}"))
                .len(),
            Channel::ALL.len()
        );
    }
    drop(store);

    let mut reopened = PrincipalStore::open(&root, retention_uniform(1), digest)
        .unwrap_or_else(|error| panic!("reopen: {error}"));
    {
        let mut scope = reopened
            .principal(&principal("alice"))
            .unwrap_or_else(|error| panic!("reopened alice: {error}"));
        let mut audit =
            AuditChain::open(&scope).unwrap_or_else(|error| panic!("reopened audit: {error}"));
        let restarted = Dispatcher::dispatch(&mut scope, &mut audit, 23, &trace, &event)
            .unwrap_or_else(|error| panic!("restart dispatch: {error}"));
        assert_eq!(restarted.outcome(), DispatchOutcome::Deduplicated);
        scope
            .expire(u64::MAX)
            .unwrap_or_else(|error| panic!("expire: {error}"));
        assert_eq!(
            Dispatcher::deliveries(&scope)
                .unwrap_or_else(|error| panic!("retained deliveries: {error}"))
                .len(),
            Channel::ALL.len()
        );
    }
    {
        let mut scope = reopened
            .principal(&principal("bob"))
            .unwrap_or_else(|error| panic!("bob scope: {error}"));
        let mut audit =
            AuditChain::open(&scope).unwrap_or_else(|error| panic!("bob audit: {error}"));
        let bob = Dispatcher::dispatch(&mut scope, &mut audit, 24, &trace, &event)
            .unwrap_or_else(|error| panic!("bob dispatch: {error}"));
        assert_eq!(bob.outcome(), DispatchOutcome::Dispatched);
        assert_eq!(bob.deliveries().len(), Channel::ALL.len());
    }
    fs::remove_dir_all(root).unwrap_or_else(|error| panic!("cleanup: {error}"));
}
