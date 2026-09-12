use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use layerx_platform_internal::journal::{Journal, LOCK_REFUSAL};
use layerx_platform_internal::producer::{
    Client, Health, Observation, Outbox, Pending, QueueState,
};
use serde::{Deserialize, Serialize};

/// Longest a caller waits for the advisory journal lock another holder owns.
/// The delivery worker polls the same directory once a second and the request
/// worker runs in its own process, so a verified publication must outwait the
/// replay of a neighbouring holder rather than be refused.
const LOCK_WAIT: Duration = Duration::from_secs(5);
/// Pause between attempts to take a contended journal lock.
const LOCK_RETRY: Duration = Duration::from_millis(10);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
enum Entry {
    Enqueue {
        transition: String,
        observation: Box<Observation>,
    },
    Acknowledge {
        id: String,
        observed: bool,
    },
    Unbound,
    Overflow,
}

pub struct ProgramOutbox {
    directory: PathBuf,
    gate: Mutex<()>,
    pub health: Arc<Health>,
}

impl ProgramOutbox {
    /// # Errors
    /// Refuses malformed verified publication fields or unavailable durable state.
    pub fn enqueue_publication(
        &self,
        verified_body: &str,
        principal: &str,
        now: u64,
    ) -> Result<(), String> {
        let body: serde_json::Value =
            serde_json::from_str(verified_body).map_err(|_| "verified program encoding invalid")?;
        let resource = body["program_id"]
            .as_str()
            .ok_or("verified program identity missing")?;
        let version = body["versions"]
            .as_array()
            .and_then(|versions| versions.last())
            .ok_or("verified program version missing")?;
        let source_sequence = version["version"]
            .as_u64()
            .ok_or("verified program version invalid")?;
        let code_hash = version["code_hash"]
            .as_str()
            .ok_or("verified code hash missing")?;
        let receipt_digest = version["deployment_receipt_digest"]
            .as_str()
            .ok_or("verified receipt digest missing")?;
        let lifecycle = body["lifecycle"]
            .as_str()
            .ok_or("verified lifecycle missing")?;
        self.enqueue(
            &format!("{resource}:{source_sequence}"),
            layerx_platform_internal::producer::Observation {
                kind: "program".to_owned(),
                id: String::new(),
                principal: None,
                principal_digest: Some(principal.to_owned()),
                resource: resource.to_owned(),
                sequence: 0,
                source_sequence,
                occurred_at: now,
                facts: [
                    ("version", source_sequence.to_string()),
                    ("code_hash", code_hash.to_owned()),
                    ("receipt_digest", receipt_digest.to_owned()),
                    ("lifecycle", lifecycle.to_owned()),
                ]
                .into_iter()
                .map(|(name, value)| layerx_platform_internal::events::Fact {
                    name: name.to_owned(),
                    value,
                })
                .collect(),
                activity_id: None,
                amount: None,
                asset: None,
            },
        )
    }

    #[must_use]
    pub fn new(directory: &Path) -> Self {
        Self {
            directory: directory.join("event-outbox"),
            gate: Mutex::new(()),
            health: Arc::new(Health::default()),
        }
    }

    /// Runs one journal operation while this process holds the outbox to
    /// itself, retrying the exclusive open for [`LOCK_WAIT`] while another
    /// holder owns the advisory lock. The journal is closed, and the lock
    /// released, before the caller sees the answer.
    fn with_journal<T>(
        &self,
        action: impl FnOnce(&mut Journal, QueueState, u64, u64) -> Result<T, String>,
    ) -> Result<T, String> {
        let _gate = self
            .gate
            .lock()
            .map_err(|_| "program journal gate is unavailable".to_owned())?;
        let deadline = Instant::now()
            .checked_add(LOCK_WAIT)
            .ok_or("program journal deadline is invalid")?;
        let (mut journal, state, unbound, overflow) = loop {
            match self.open() {
                Err(error) if error.starts_with(LOCK_REFUSAL) && Instant::now() < deadline => {
                    std::thread::sleep(LOCK_RETRY);
                }
                result => break result?,
            }
        };
        action(&mut journal, state, unbound, overflow)
    }

    fn open(&self) -> Result<(Journal, QueueState, u64, u64), String> {
        let mut entries = Vec::new();
        let journal = Journal::open::<Entry>(&self.directory, |entry| entries.push(entry))?;
        let mut state = QueueState::default();
        let mut unbound = 0_u64;
        let mut overflow = 0_u64;
        for entry in entries {
            match entry {
                Entry::Enqueue {
                    transition,
                    observation,
                } => {
                    let observation = *observation;
                    observation.validate()?;
                    if state.enqueue(&transition, observation.clone())? != observation {
                        return Err("program journal counter mismatch".to_owned());
                    }
                }
                Entry::Acknowledge { id, observed } => state.acknowledge(&id, observed)?,
                Entry::Overflow => {
                    overflow = overflow
                        .checked_add(1)
                        .ok_or("program overflow counter exhausted")?;
                }
                Entry::Unbound => {
                    unbound = unbound
                        .checked_add(1)
                        .ok_or("program unbound counter exhausted")?;
                }
            }
        }
        state.validate()?;
        Ok((journal, state, unbound, overflow))
    }

    /// # Errors
    /// Refuses an unavailable or full journal and changed publication retries.
    pub fn enqueue(&self, transition: &str, observation: Observation) -> Result<(), String> {
        self.with_journal(move |journal, mut state, _, _| {
            let mut observation = observation;
            if let Some(previous) = state.retained(transition) {
                observation.sequence = previous.sequence;
                observation.id.clone_from(&previous.id);
                observation.occurred_at = previous.occurred_at;
                return if observation == *previous {
                    Ok(())
                } else {
                    Err("publication retry differs".to_owned())
                };
            }
            if state.full() {
                journal.append(&Entry::Overflow)?;
                self.health.overflow();
                return Err("program event queue full".to_owned());
            }
            let observation = state.enqueue(transition, observation)?;
            journal.append(&Entry::Enqueue {
                transition: transition.to_owned(),
                observation: Box::new(observation),
            })
        })
    }

    /// # Errors
    /// Refuses failure to durably count an unresolved publication principal.
    pub fn unbound(&self) -> Result<(), String> {
        self.with_journal(|journal, _, _, _| journal.append(&Entry::Unbound))
    }

    /// # Errors
    /// Refuses unavailable durable metrics.
    pub fn unbound_count(&self) -> Result<u64, String> {
        self.with_journal(|_, _, count, _| Ok(count))
    }

    /// # Errors
    /// Refuses unavailable durable metrics.
    pub fn overflow_count(&self) -> Result<u64, String> {
        self.with_journal(|_, _, _, count| Ok(count))
    }

    /// # Errors
    /// Refuses incomplete producer credentials or an unavailable journal.
    pub fn start(self: &Arc<Self>) -> Result<(), String> {
        self.with_journal(|_, _, _, _| Ok(()))?;
        Client::from_environment(&["program"])?
            .spawn(Arc::downgrade(self), Arc::clone(&self.health))
    }
}

impl Outbox for ProgramOutbox {
    fn pending(&self) -> Result<Option<Pending>, String> {
        self.with_journal(|_, state, _, _| Ok(state.pending()))
    }
    fn acknowledge(&self, id: &str, observed: bool) -> Result<(), String> {
        self.with_journal(|journal, mut state, _, _| {
            state.acknowledge(id, observed)?;
            journal.append(&Entry::Acknowledge {
                id: id.to_owned(),
                observed,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layerx_platform_internal::events::Fact;

    #[test]
    fn program_queue_replays_counters_payloads_acknowledgements_and_unbound_count() {
        let directory =
            std::env::temp_dir().join(format!("registry-event-outbox-{}", std::process::id()));
        let outbox = ProgramOutbox::new(&directory);
        let observation = Observation {
            kind: "program".to_owned(),
            id: String::new(),
            principal: None,
            principal_digest: Some("ab".repeat(32)),
            resource: "cd".repeat(32),
            sequence: 0,
            source_sequence: 7,
            occurred_at: 100,
            facts: vec![Fact {
                name: "version".to_owned(),
                value: "7".to_owned(),
            }],
            activity_id: None,
            amount: None,
            asset: None,
        };
        assert!(outbox
            .enqueue("publication-seven", observation.clone())
            .is_ok());
        assert!(outbox.unbound().is_ok());
        let pending = outbox
            .pending()
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("queue empty"));
        assert_eq!(pending.observation.sequence, 1);
        assert_eq!(pending.observation.source_sequence, 7);
        assert!(outbox.acknowledge(&pending.observation.id, false).is_err());
        assert!(outbox.acknowledge(&pending.observation.id, true).is_ok());
        drop(outbox);
        let outbox = ProgramOutbox::new(&directory);
        assert_eq!(
            outbox
                .unbound_count()
                .unwrap_or_else(|error| panic!("{error}")),
            1
        );
        let mut observed = pending.clone();
        observed.observed = true;
        assert_eq!(
            outbox.pending().unwrap_or_else(|error| panic!("{error}")),
            Some(observed)
        );
        assert!(outbox.acknowledge(&pending.observation.id, false).is_ok());
        assert!(outbox
            .enqueue("publication-seven", observation.clone())
            .is_ok());
        assert!(outbox
            .pending()
            .unwrap_or_else(|error| panic!("{error}"))
            .is_none());
        let mut changed = observation;
        changed.principal_digest = Some("ef".repeat(32));
        assert!(outbox
            .enqueue("publication-seven", changed.clone())
            .is_err());
        changed.principal_digest = Some("ab".repeat(32));
        changed.source_sequence = 9;
        assert!(outbox.enqueue("publication-nine", changed).is_ok());
        assert_eq!(
            outbox
                .pending()
                .unwrap_or_else(|error| panic!("{error}"))
                .map(|entry| entry.observation.sequence),
            Some(2)
        );
        drop(outbox);
        std::fs::remove_dir_all(directory).unwrap_or_else(|error| panic!("{error}"));
    }
}
