use std::sync::{Arc, Mutex, OnceLock};

use layerx_platform_internal::producer::{Health, Observation, Outbox, Pending, QueueState};

use crate::store::{PrincipalScope, PrincipalStore, RowKey, StoreError, Table};

static HEALTH: OnceLock<Arc<Health>> = OnceLock::new();

pub(crate) fn health() -> Arc<Health> {
    Arc::clone(HEALTH.get_or_init(|| Arc::new(Health::default())))
}

fn key() -> Result<RowKey, StoreError> {
    RowKey::new("event-producer")
}

fn state(scope: &PrincipalScope<'_>) -> Result<QueueState, StoreError> {
    let state = scope
        .get(Table::EventOutbox, &key()?)
        .map(|row| {
            serde_json::from_slice::<QueueState>(row.bytes())
                .map_err(|_| StoreError::Corrupt("invalid event outbox"))
        })
        .transpose()?
        .unwrap_or_default();
    state
        .validate()
        .map_err(|_| StoreError::Corrupt("invalid event outbox identity"))?;
    Ok(state)
}

pub(crate) fn enqueue_row(
    scope: &PrincipalScope<'_>,
    transition: &str,
    mut observation: Observation,
) -> Result<(Table, RowKey, Vec<u8>), StoreError> {
    let mut state = state(scope)?;
    if let Some(previous) = state.retained(transition) {
        observation.source_sequence = previous.source_sequence;
        observation.occurred_at = previous.occurred_at;
    }
    let result = state.enqueue(transition, observation);
    if result.is_err() && state.full() {
        health().overflow();
    }
    result.map_err(|_| StoreError::Corrupt("event enqueue refused"))?;
    let bytes =
        serde_json::to_vec(&state).map_err(|_| StoreError::Corrupt("event outbox encoding"))?;
    Ok((Table::EventOutbox, key()?, bytes))
}

pub(crate) struct HumanOutbox {
    store: Arc<Mutex<PrincipalStore>>,
    health: Arc<Health>,
}

impl HumanOutbox {
    pub(crate) fn ready(&self) -> bool {
        self.health.ready()
    }
    pub(crate) fn start(store: Arc<Mutex<PrincipalStore>>) -> Result<Arc<Self>, String> {
        let outbox = Arc::new(Self {
            store,
            health: health(),
        });
        layerx_platform_internal::producer::Client::from_environment(&["journey", "approval"])?
            .spawn(Arc::downgrade(&outbox), health())?;
        Ok(outbox)
    }
}

impl Outbox for HumanOutbox {
    fn pending(&self) -> Result<Option<Pending>, String> {
        let mut store = self.store.lock().map_err(|_| "Human store unavailable")?;
        for principal in store.tenancy().principals() {
            let scope = store
                .principal(&principal)
                .map_err(|error| error.to_string())?;
            if let Some(pending) = state(&scope).map_err(|error| error.to_string())?.pending() {
                return Ok(Some(pending));
            }
        }
        Ok(None)
    }

    fn acknowledge(&self, id: &str, observed: bool) -> Result<(), String> {
        let mut store = self.store.lock().map_err(|_| "Human store unavailable")?;
        for principal in store.tenancy().principals() {
            let mut scope = store
                .principal(&principal)
                .map_err(|error| error.to_string())?;
            let mut state = state(&scope).map_err(|error| error.to_string())?;
            if let Some(pending) = state.pending().filter(|entry| entry.observation.id == id) {
                state.acknowledge(id, observed)?;
                let bytes = serde_json::to_vec(&state).map_err(|error| error.to_string())?;
                return scope
                    .put(
                        Table::EventOutbox,
                        key().map_err(|error| error.to_string())?,
                        pending.observation.occurred_at,
                        bytes,
                    )
                    .map_err(|error| error.to_string());
            }
        }
        Err("Human acknowledgement has no pending fact".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{AgentTenantId, PrincipalId, RetentionPeriod, RetentionPolicy, TenancyMap};

    #[test]
    fn stream_and_producer_entries_persist_together_and_outlive_delivery_restarts() {
        let directory =
            std::env::temp_dir().join(format!("human-event-outbox-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap_or_else(|error| panic!("{error}"));
        let principal = PrincipalId::new("alice").unwrap_or_else(|error| panic!("{error}"));
        let other = PrincipalId::new("bob").unwrap_or_else(|error| panic!("{error}"));
        let tenant = AgentTenantId::new("tenant-alice").unwrap_or_else(|error| panic!("{error}"));
        let map = TenancyMap::new([(principal.clone(), tenant.clone()), (other.clone(), tenant)])
            .unwrap_or_else(|error| panic!("{error}"));
        let digest = map
            .install(&directory)
            .unwrap_or_else(|error| panic!("{error}"));
        let period = RetentionPeriod::new(10);
        let retention = RetentionPolicy {
            journeys: period,
            notifications: period,
            audit: period,
            telemetry: period,
            cache: period,
        };
        let store = Arc::new(Mutex::new(
            PrincipalStore::open(&directory, retention, digest)
                .unwrap_or_else(|error| panic!("{error}")),
        ));
        let outbox = HumanOutbox {
            store: Arc::clone(&store),
            health: health(),
        };
        let journal = crate::server::stream_journal::StreamJournal::new(
            [1; 32],
            layerx_proof::checkpoint::SettlementDomain::new(31337, [2; 20]),
        );
        {
            let mut store = store.lock().unwrap_or_else(|error| panic!("{error}"));
            let mut scope = store
                .principal(&principal)
                .unwrap_or_else(|error| panic!("{error}"));
            journal
                .append(
                    &mut scope,
                    "notification:one",
                    "notification",
                    100,
                    serde_json::json!({"notification":{}}),
                )
                .unwrap_or_else(|_| panic!("notification append"));
            let body = serde_json::json!({"approval":{"approval_id":"approval-one","agent_id":"agent-one","state":"pending","created_at":100}});
            journal
                .append(
                    &mut scope,
                    "approval:one:pending",
                    "approval-created",
                    101,
                    body.clone(),
                )
                .unwrap_or_else(|_| panic!("approval append"));
            journal
                .append(
                    &mut scope,
                    "approval:one:pending",
                    "approval-created",
                    101,
                    body,
                )
                .unwrap_or_else(|_| panic!("approval retry"));
            assert_eq!(
                state(&scope)
                    .unwrap_or_else(|error| panic!("{error}"))
                    .pending()
                    .map(|entry| (
                        entry.observation.sequence,
                        entry.observation.source_sequence
                    )),
                Some((1, 2))
            );
            let foreign = store
                .principal(&other)
                .unwrap_or_else(|error| panic!("{error}"));
            assert!(state(&foreign)
                .unwrap_or_else(|error| panic!("{error}"))
                .pending()
                .is_none());
        }
        assert_restart(directory, store, outbox, retention, digest, &principal);
    }

    fn assert_restart(
        directory: std::path::PathBuf,
        store: Arc<Mutex<PrincipalStore>>,
        outbox: HumanOutbox,
        retention: RetentionPolicy,
        digest: crate::store::TenancyDigest,
        principal: &PrincipalId,
    ) {
        let pending = outbox
            .pending()
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("pending missing"));
        assert!(outbox.acknowledge(&pending.observation.id, false).is_err());
        assert!(outbox.acknowledge(&pending.observation.id, true).is_ok());
        drop(outbox);
        drop(store);
        let store = Arc::new(Mutex::new(
            PrincipalStore::open(&directory, retention, digest)
                .unwrap_or_else(|error| panic!("{error}")),
        ));
        let outbox = HumanOutbox {
            store: Arc::clone(&store),
            health: health(),
        };
        let mut observed = pending.clone();
        observed.observed = true;
        assert_eq!(
            outbox.pending().unwrap_or_else(|error| panic!("{error}")),
            Some(observed)
        );
        {
            let mut store = store.lock().unwrap_or_else(|error| panic!("{error}"));
            let mut scope = store
                .principal(principal)
                .unwrap_or_else(|error| panic!("{error}"));
            scope.expire(200).unwrap_or_else(|error| panic!("{error}"));
            assert!(state(&scope)
                .unwrap_or_else(|error| panic!("{error}"))
                .pending()
                .is_some());
        }
        assert!(outbox.acknowledge(&pending.observation.id, false).is_ok());
        assert!(outbox
            .pending()
            .unwrap_or_else(|error| panic!("{error}"))
            .is_none());
        drop(outbox);
        drop(store);
        std::fs::remove_dir_all(directory).unwrap_or_else(|error| panic!("{error}"));
    }
}

#[cfg(test)]
#[path = "event_producer_tls.rs"]
mod tls_tests;
