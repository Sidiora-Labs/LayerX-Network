use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::events::{Fact, Record};
use crate::secret::{sha256_hex, valid_hex, valid_identifier, valid_principal};
use crate::tls::Upstream;

pub const MAX_PENDING: usize = 1024;
pub const MAX_OBSERVATION_BYTES: usize = 16 * 1024;
const UNAVAILABLE_AFTER: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub kind: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub principal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub principal_digest: Option<String>,
    pub resource: String,
    pub sequence: u64,
    pub source_sequence: u64,
    pub occurred_at: u64,
    pub facts: Vec<Fact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset: Option<String>,
}

#[must_use]
pub fn event_id(kind: &str, resource: &str, sequence: u64) -> String {
    let mut bytes = Vec::new();
    for part in [
        kind.as_bytes(),
        resource.as_bytes(),
        &sequence.to_be_bytes(),
    ] {
        bytes.extend_from_slice(&(part.len() as u64).to_be_bytes());
        bytes.extend_from_slice(part);
    }
    sha256_hex(&bytes)
}

impl Observation {
    /// # Errors
    /// Refuses invalid identities, ambiguous principals and oversized facts.
    pub fn validate(&self) -> Result<(), String> {
        let principal_valid = match (&self.principal, &self.principal_digest) {
            (Some(principal), None) => valid_principal(principal),
            (None, Some(digest)) => {
                matches!(self.kind.as_str(), "payment" | "program") && valid_hex(digest, 32)
            }
            _ => false,
        };
        if !matches!(
            self.kind.as_str(),
            "payment" | "program" | "journey" | "approval"
        ) || !principal_valid
            || !valid_identifier(&self.resource, 128)
            || self.sequence == 0
            || self.occurred_at == 0
            || self.id != event_id(&self.kind, &self.resource, self.sequence)
            || self.facts.is_empty()
            || self.facts.len() >= 32
            || self.facts.iter().any(|fact| {
                fact.name == "source_sequence"
                    || !valid_identifier(&fact.name, 128)
                    || fact.value.len() > 512
                    || fact.value.contains(['\0', '\n', '\r'])
            })
            || self
                .activity_id
                .as_ref()
                .is_some_and(|id| !valid_hex(id, 32))
        {
            return Err("invalid producer observation".to_owned());
        }
        if self.kind == "payment"
            && (self.activity_id.as_deref() != Some(self.resource.as_str())
                || self.amount.as_ref().is_none_or(|amount| {
                    amount.is_empty() || amount.len() > 39 || amount.parse::<u128>().is_err()
                })
                || self
                    .asset
                    .as_ref()
                    .is_none_or(|asset| !valid_hex(asset, 32)))
        {
            return Err("invalid payment observation".to_owned());
        }
        Ok(())
    }

    #[must_use]
    pub fn record(&self, principal: String) -> Record {
        let mut facts = self.facts.clone();
        facts.push(Fact {
            name: "source_sequence".to_owned(),
            value: self.source_sequence.to_string(),
        });
        Record {
            id: self.id.clone(),
            principal,
            subject: self.resource.clone(),
            subject_sequence: self.sequence,
            occurred_at: self.occurred_at,
            facts,
            activity_id: self.activity_id.clone(),
            amount: self.amount.clone(),
            asset: self.asset.clone(),
        }
    }

    /// # Errors
    /// Refuses invalid or oversized observations.
    pub fn encode(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|error| error.to_string())?;
        if bytes.len() > MAX_OBSERVATION_BYTES {
            return Err("producer observation exceeds bound".to_owned());
        }
        Ok(bytes)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Pending {
    pub observation: Observation,
    pub body: Vec<u8>,
    pub observed: bool,
}

impl Pending {
    /// # Errors
    /// Refuses invalid or oversized observations.
    pub fn new(observation: Observation) -> Result<Self, String> {
        let body = observation.encode()?;
        Ok(Self {
            observation,
            body,
            observed: false,
        })
    }

    /// # Errors
    /// Refuses a queued body that differs from its complete observation.
    pub fn validate(&self) -> Result<(), String> {
        if self.body != self.observation.encode()? {
            return Err("producer queue body mismatch".to_owned());
        }
        Ok(())
    }
}

pub trait Outbox: Send + Sync + 'static {
    /// # Errors
    /// Refuses unreadable or corrupt durable queue state.
    fn pending(&self) -> Result<Option<Pending>, String>;
    /// # Errors
    /// Returns a durable acknowledgement failure.
    fn acknowledge(&self, id: &str, observed: bool) -> Result<(), String>;
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueState {
    counters: BTreeMap<String, u64>,
    entries: Vec<Pending>,
    retained: BTreeMap<String, Observation>,
}

impl QueueState {
    /// # Errors
    /// Refuses corrupt entries, identities and non-contiguous counters.
    pub fn validate(&self) -> Result<(), String> {
        if self.entries.len() > MAX_PENDING {
            return Err("producer queue exceeds bound".to_owned());
        }
        let mut sequences = BTreeMap::<String, Vec<u64>>::new();
        for (transition, observation) in &self.retained {
            observation.validate()?;
            if transition.is_empty() {
                return Err("producer transition missing".to_owned());
            }
            sequences
                .entry(format!("{}:{}", observation.kind, observation.resource))
                .or_default()
                .push(observation.sequence);
        }
        if sequences.len() != self.counters.len() {
            return Err("producer counter mismatch".to_owned());
        }
        for (resource, mut values) in sequences {
            values.sort_unstable();
            if values
                .iter()
                .enumerate()
                .any(|(index, value)| *value != index as u64 + 1)
                || self.counters.get(&resource) != values.last()
            {
                return Err("producer sequence mismatch".to_owned());
            }
        }
        let mut ids = std::collections::BTreeSet::new();
        for pending in &self.entries {
            pending.validate()?;
            if !ids.insert(&pending.observation.id)
                || !self
                    .retained
                    .values()
                    .any(|entry| *entry == pending.observation)
            {
                return Err("producer pending identity mismatch".to_owned());
            }
        }
        Ok(())
    }

    /// # Errors
    /// Refuses changed retries, full queues and exhausted resource sequences.
    pub fn enqueue(
        &mut self,
        transition: &str,
        mut observation: Observation,
    ) -> Result<Observation, String> {
        if let Some(previous) = self.retained.get(transition) {
            observation.sequence = previous.sequence;
            observation.id.clone_from(&previous.id);
            return if *previous == observation {
                Ok(previous.clone())
            } else {
                Err("producer transition conflicts".to_owned())
            };
        }
        if self.entries.len() >= MAX_PENDING {
            return Err("producer queue full".to_owned());
        }
        let resource = format!("{}:{}", observation.kind, observation.resource);
        observation.sequence = self
            .counters
            .get(&resource)
            .copied()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| "producer sequence exhausted".to_owned())?;
        observation.id = event_id(
            &observation.kind,
            &observation.resource,
            observation.sequence,
        );
        let pending = Pending::new(observation.clone())?;
        self.counters.insert(resource, observation.sequence);
        self.retained
            .insert(transition.to_owned(), observation.clone());
        self.entries.push(pending);
        Ok(observation)
    }

    #[must_use]
    pub fn pending(&self) -> Option<Pending> {
        self.entries.first().cloned()
    }

    #[must_use]
    pub fn retained(&self, transition: &str) -> Option<&Observation> {
        self.retained.get(transition)
    }

    #[must_use]
    pub fn full(&self) -> bool {
        self.entries.len() >= MAX_PENDING
    }

    /// # Errors
    /// Refuses acknowledgements outside the durable queue order.
    pub fn acknowledge(&mut self, id: &str, observed: bool) -> Result<(), String> {
        let first = self
            .entries
            .first_mut()
            .ok_or_else(|| "producer queue empty".to_owned())?;
        if first.observation.id != id || first.observed == observed {
            return Err("producer acknowledgement out of order".to_owned());
        }
        if observed {
            first.observed = true;
        } else {
            self.entries.remove(0);
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct Health {
    failed_since: Mutex<Option<Instant>>,
    failures: AtomicU64,
    overflow: AtomicU64,
}

impl Health {
    pub fn overflow(&self) {
        self.overflow.fetch_add(1, Ordering::Relaxed);
        self.failed();
        eprintln!("event_producer_queue_overflow");
    }

    fn failed(&self) {
        self.failures.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut since) = self.failed_since.lock() {
            since.get_or_insert_with(Instant::now);
        }
    }

    fn recovered(&self) {
        if let Ok(mut since) = self.failed_since.lock() {
            *since = None;
        }
    }

    #[must_use]
    pub fn ready(&self) -> bool {
        self.failed_since
            .lock()
            .is_ok_and(|since| since.is_none_or(|since| since.elapsed() < UNAVAILABLE_AFTER))
    }

    #[must_use]
    pub fn metrics(&self) -> (u64, u64) {
        (
            self.failures.load(Ordering::Relaxed),
            self.overflow.load(Ordering::Relaxed),
        )
    }
}

pub struct Client {
    sources: BTreeMap<String, Upstream>,
    webhooks: Upstream,
}

impl Client {
    /// # Errors
    /// Requires mutually authenticated source and webhook clients.
    pub fn new(sources: BTreeMap<String, Upstream>, webhooks: Upstream) -> Result<Self, String> {
        if sources.is_empty()
            || !webhooks.authenticated_producer()
            || sources.iter().any(|(kind, source)| {
                !matches!(
                    kind.as_str(),
                    "payment" | "program" | "journey" | "approval"
                ) || !source.authenticated_producer()
            })
        {
            return Err("producer requires mTLS and bearer credentials".to_owned());
        }
        Ok(Self { sources, webhooks })
    }

    /// # Errors
    /// Refuses missing per-kind TLS clients or webhook configuration.
    pub fn from_environment(kinds: &[&str]) -> Result<Self, String> {
        let mut sources = BTreeMap::new();
        for kind in kinds {
            let prefix = format!("LAYERX_EVENTS_{}", kind.to_ascii_uppercase());
            sources.insert((*kind).to_owned(), Upstream::from_environment(&prefix)?);
        }
        Self::new(
            sources,
            Upstream::from_environment("LAYERX_EVENTS_WEBHOOKS")?,
        )
    }

    /// # Errors
    /// Returns observation or notification failures without discarding the queue entry.
    pub fn deliver(&self, pending: &Pending) -> Result<bool, String> {
        pending.validate()?;
        let observation = &pending.observation;
        if !pending.observed {
            let source = self
                .sources
                .get(&observation.kind)
                .ok_or_else(|| "producer source missing".to_owned())?;
            let response = source
                .post("/internal/v1/observe", &pending.body)
                .map_err(|_| "event observation unavailable".to_owned())?;
            if response.status != 200 || !response.content_type.starts_with("application/json") {
                return Err(format!("event observation refused: {}", response.status));
            }
            let record: Record = serde_json::from_slice(&response.body)
                .map_err(|_| "event acknowledgement malformed".to_owned())?;
            let principal = observation
                .principal
                .clone()
                .unwrap_or_else(|| record.principal.clone());
            if observation
                .principal_digest
                .as_ref()
                .is_some_and(|digest| sha256_hex(principal.as_bytes()) != *digest)
                || record != observation.record(principal)
            {
                return Err("event acknowledgement mismatch".to_owned());
            }
            return Ok(true);
        }
        let path = format!(
            "/internal/v1/events/{}/{}",
            observation.kind, observation.id
        );
        let response = self
            .webhooks
            .post(&path, b"{}")
            .map_err(|_| "event notification unavailable".to_owned())?;
        if response.status != 202 {
            return Err(format!("event notification refused: {}", response.status));
        }
        Ok(false)
    }

    /// # Errors
    /// Retains the pending entry and updates health when one delivery attempt fails.
    pub fn step<S: Outbox>(&self, store: &S, health: &Health) -> Result<(), String> {
        let result = store.pending().and_then(|pending| {
            if let Some(pending) = pending {
                let observed = self.deliver(&pending)?;
                store.acknowledge(&pending.observation.id, observed)?;
            } else {
                for source in self.sources.values() {
                    if !source
                        .get("/livez")
                        .is_ok_and(|response| response.status == 200)
                    {
                        return Err("event source unreachable".to_owned());
                    }
                }
            }
            Ok(())
        });
        match &result {
            Ok(()) => health.recovered(),
            Err(error) => {
                health.failed();
                eprintln!("event_producer_delivery_failure: {error}");
            }
        }
        result
    }

    /// # Errors
    /// Returns failure to start the delivery worker.
    pub fn spawn<S: Outbox>(self, store: Weak<S>, health: Arc<Health>) -> Result<(), String> {
        std::thread::Builder::new()
            .name("event-producer".to_owned())
            .spawn(move || {
                while let Some(store) = store.upgrade() {
                    let _ = self.step(store.as_ref(), &health);
                    drop(store);
                    std::thread::sleep(Duration::from_secs(1));
                }
            })
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn observation(sequence: u64) -> Observation {
        Observation {
            kind: "journey".to_owned(),
            id: event_id("journey", "journey-one", sequence),
            principal: Some("principal-one".to_owned()),
            principal_digest: None,
            resource: "journey-one".to_owned(),
            sequence,
            source_sequence: 17,
            occurred_at: 123,
            facts: vec![Fact {
                name: "state".to_owned(),
                value: "processing".to_owned(),
            }],
            activity_id: None,
            amount: None,
            asset: None,
        }
    }

    #[test]
    fn durable_payload_cannot_be_substituted_or_reidentified() {
        let first = observation(1);
        assert_ne!(first.id, observation(2).id);
        assert_ne!(first.id, event_id("approval", "journey-one", 1));
        assert_ne!(first.id, event_id("journey", "journey-two", 1));
        let mut pending = Pending::new(first.clone()).unwrap_or_else(|error| panic!("{error}"));
        assert!(pending.validate().is_ok());
        pending.observed = true;
        assert!(pending.validate().is_ok());
        pending.observation.facts[0].value = "refused".to_owned();
        assert!(pending.validate().is_err());
        let mut changed = first;
        changed.sequence = 2;
        assert!(changed.validate().is_err());
        changed = observation(1);
        changed.principal_digest = Some("a".repeat(64));
        assert!(changed.validate().is_err());
        changed.principal = None;
        assert!(changed.validate().is_err());
        changed = observation(0);
        assert!(changed.validate().is_err());
        changed = observation(1);
        changed.facts[0].value = "x".repeat(513);
        assert!(changed.validate().is_err());
    }

    #[test]
    fn unreachable_threshold_refuses_readiness_and_tracks_overflow() {
        let health = Health::default();
        assert!(health.ready());
        health.failed();
        assert!(health.ready());
        *health
            .failed_since
            .lock()
            .unwrap_or_else(|error| panic!("{error}")) = Some(
            Instant::now()
                .checked_sub(UNAVAILABLE_AFTER)
                .unwrap_or_else(|| panic!("readiness clock underflow")),
        );
        assert!(!health.ready());
        health.overflow();
        assert_eq!(health.metrics(), (2, 1));
        health.recovered();
        assert!(health.ready());
    }
}

#[cfg(test)]
mod queue_tests {
    use super::*;

    #[test]
    fn counters_are_per_resource_and_retries_preserve_complete_entries() {
        let mut queue = QueueState::default();
        let mut one = super::tests::observation(1);
        one.source_sequence = 57;
        let first = queue
            .enqueue("journal-57", one.clone())
            .unwrap_or_else(|error| panic!("{error}"));
        let mut two = one.clone();
        two.source_sequence = 83;
        let second = queue
            .enqueue("journal-83", two)
            .unwrap_or_else(|error| panic!("{error}"));
        let mut other = one.clone();
        other.resource = "journey-two".to_owned();
        other.source_sequence = 84;
        let other = queue
            .enqueue("journal-84", other)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!((first.sequence, second.sequence, other.sequence), (1, 2, 1));
        assert_eq!((first.source_sequence, second.source_sequence), (57, 83));
        assert_eq!(queue.enqueue("journal-57", one.clone()), Ok(first.clone()));
        let encoded = serde_json::to_vec(&queue).unwrap_or_else(|error| panic!("{error}"));
        let mut queue: QueueState =
            serde_json::from_slice(&encoded).unwrap_or_else(|error| panic!("{error}"));
        assert!(queue.validate().is_ok());
        assert!(queue.acknowledge(&second.id, true).is_err());
        assert!(queue.acknowledge(&first.id, false).is_err());
        assert!(queue.acknowledge(&first.id, true).is_ok());
        assert!(queue.acknowledge(&first.id, false).is_ok());
        assert_eq!(queue.enqueue("journal-57", one.clone()), Ok(first));
        assert_eq!(queue.pending().map(|entry| entry.observation), Some(second));
        one.facts[0].value = "changed".to_owned();
        assert!(queue.enqueue("journal-57", one).is_err());
    }

    #[test]
    fn overflow_does_not_consume_a_resource_sequence() {
        let mut queue = QueueState::default();
        for sequence in 1..=MAX_PENDING {
            let mut observation = super::tests::observation(1);
            observation.source_sequence = sequence as u64;
            assert!(queue
                .enqueue(&format!("transition-{sequence}"), observation)
                .is_ok());
        }
        let candidate = super::tests::observation(1);
        assert!(queue.enqueue("after-full", candidate.clone()).is_err());
        let head = queue.pending().unwrap_or_else(|| panic!("queue empty"));
        assert!(queue.acknowledge(&head.observation.id, true).is_ok());
        assert!(queue.acknowledge(&head.observation.id, false).is_ok());
        let next = queue
            .enqueue("after-full", candidate)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(next.sequence, MAX_PENDING as u64 + 1);
        assert!(queue.validate().is_ok());
    }
}
