use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use zeroize::Zeroizing;

use crate::http::{json, ok, refusal, Request, Response};
use crate::journal::Journal;
use crate::secret::{
    hex, sha256_hex, unhex, unix_seconds, valid_hex, valid_identifier, valid_principal,
};
use crate::tls::Upstream;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Journey,
    Approval,
    Payment,
    Program,
}

impl Kind {
    /// Parses the four source families.
    /// # Errors
    /// Refuses any undeclared family.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "journeys" => Ok(Self::Journey),
            "approvals" => Ok(Self::Approval),
            "payments" => Ok(Self::Payment),
            "programs" => Ok(Self::Program),
            _ => Err("invalid source kind".to_owned()),
        }
    }
    #[must_use]
    pub const fn singular(self) -> &'static str {
        match self {
            Self::Journey => "journey",
            Self::Approval => "approval",
            Self::Payment => "payment",
            Self::Program => "program",
        }
    }
    fn route(self, resource: &str) -> String {
        let prefix = match self {
            Self::Journey => "/v1/journeys",
            Self::Approval => "/v1/approvals",
            Self::Payment => "/v1/receipts",
            Self::Program => "/v1/programs/registry",
        };
        format!("{prefix}/{resource}")
    }
    fn credential(self, value: &str) -> (&'static str, Zeroizing<String>) {
        if matches!(self, Self::Journey | Self::Approval) {
            (
                "Cookie",
                Zeroizing::new(format!("__Host-layerx_access={value}")),
            )
        } else {
            (
                "Authorization",
                Zeroizing::new(format!("LayerX-Key {value}")),
            )
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Fact {
    pub name: String,
    pub value: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub id: String,
    pub principal: String,
    pub subject: String,
    pub subject_sequence: u64,
    pub occurred_at: u64,
    pub facts: Vec<Fact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum Stored {
    Producer { record: Record, observation: String },
    Legacy(Record),
}

pub struct ProducerCredential {
    pub token: Zeroizing<String>,
    pub allow_principal_digest: bool,
}

struct Store {
    journal: Journal,
    records: BTreeMap<String, Record>,
    sequences: BTreeMap<(String, String), u64>,
    observations: BTreeMap<String, Vec<u8>>,
}
impl Store {
    fn open(directory: &Path) -> Result<Self, String> {
        let mut records = Vec::new();
        let journal = Journal::open::<Stored>(directory, |record| records.push(record))?;
        let mut store = Self {
            journal,
            records: BTreeMap::new(),
            sequences: BTreeMap::new(),
            observations: BTreeMap::new(),
        };
        for stored in records {
            let record = match stored {
                Stored::Legacy(record) => record,
                Stored::Producer {
                    record,
                    observation,
                } => {
                    let decoded: crate::producer::Observation = serde_json::from_str(&observation)
                        .map_err(|_| "invalid producer journal observation".to_owned())?;
                    decoded.validate()?;
                    if decoded.record(record.principal.clone()) != record
                        || decoded
                            .principal
                            .as_ref()
                            .is_some_and(|principal| principal != &record.principal)
                        || decoded
                            .principal_digest
                            .as_ref()
                            .is_some_and(|digest| principal_digest(&record.principal) != *digest)
                    {
                        return Err("producer journal observation mismatch".to_owned());
                    }
                    store
                        .observations
                        .insert(record.id.clone(), observation.into_bytes());
                    record
                }
            };
            if !valid_hex(&record.id, 32)
                || !valid_principal(&record.principal)
                || !valid_identifier(&record.subject, 128)
                || record.facts.len() > 32
                || record.subject_sequence != store.next(&record)?
                || store.records.contains_key(&record.id)
            {
                return Err("invalid event journal ordering or identity".to_owned());
            }
            store.index(record);
        }
        Ok(store)
    }
    fn next(&self, record: &Record) -> Result<u64, String> {
        self.sequences
            .get(&(record.principal.clone(), record.subject.clone()))
            .copied()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| "event sequence exhausted".to_owned())
    }
    fn index(&mut self, record: Record) {
        self.sequences.insert(
            (record.principal.clone(), record.subject.clone()),
            record.subject_sequence,
        );
        self.records.insert(record.id.clone(), record);
    }
    fn append_produced(&mut self, record: Record, body: &[u8]) -> Result<Record, u16> {
        if let Some(previous) = self.records.get(&record.id) {
            return if self
                .observations
                .get(&record.id)
                .is_some_and(|bytes| bytes == body)
            {
                Ok(previous.clone())
            } else {
                Err(409)
            };
        }
        if record.subject_sequence != self.next(&record).map_err(|_| 503_u16)? {
            return Err(409);
        }
        self.journal
            .append(&Stored::Producer {
                record: record.clone(),
                observation: std::str::from_utf8(body).map_err(|_| 400_u16)?.to_owned(),
            })
            .map_err(|_| 503_u16)?;
        self.observations.insert(record.id.clone(), body.to_vec());
        self.index(record.clone());
        Ok(record)
    }
    fn append(&mut self, mut record: Record) -> Result<Record, String> {
        if let Some(previous) = self.records.get(&record.id) {
            return Ok(previous.clone());
        }
        record.subject_sequence = self.next(&record)?;
        self.journal.append(&record)?;
        self.index(record.clone());
        Ok(record)
    }
}

pub struct Service {
    kind: Kind,
    upstream: Upstream,
    credentials: BTreeMap<String, Zeroizing<String>>,
    token: Zeroizing<String>,
    producers: Vec<ProducerCredential>,
    store: Mutex<Store>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Observe {
    principal: String,
    resource: String,
}

impl Service {
    /// Opens a durable source for explicitly provisioned principal credentials.
    /// # Errors
    /// Refuses missing credentials, invalid principals, or an invalid journal.
    pub fn open(
        kind: Kind,
        upstream: Upstream,
        credentials: BTreeMap<String, Zeroizing<String>>,
        token: Zeroizing<String>,
        directory: &Path,
    ) -> Result<Self, String> {
        if credentials.is_empty()
            || credentials.len() > 10_000
            || credentials
                .keys()
                .any(|principal| !valid_principal(principal))
        {
            return Err("invalid source principal set".to_owned());
        }
        Ok(Self {
            kind,
            upstream,
            credentials,
            token,
            producers: Vec::new(),
            store: Mutex::new(Store::open(directory)?),
        })
    }
    /// # Errors
    /// Refuses duplicate credentials and digest authority outside the payment source.
    pub fn with_producers(mut self, producers: Vec<ProducerCredential>) -> Result<Self, String> {
        if producers.len() > 3
            || producers.iter().enumerate().any(|(index, credential)| {
                credential.token.is_empty()
                    || credential.token.as_str() == self.token.as_str()
                    || (credential.allow_principal_digest
                        && !matches!(self.kind, Kind::Payment | Kind::Program))
                    || producers[..index]
                        .iter()
                        .any(|other| other.token == credential.token)
            })
        {
            return Err("invalid producer credentials".to_owned());
        }
        self.producers = producers;
        Ok(self)
    }

    fn observe_produced(
        &self,
        body: &[u8],
        credential: &ProducerCredential,
    ) -> Result<Record, u16> {
        let observation: crate::producer::Observation =
            serde_json::from_slice(body).map_err(|_| 400_u16)?;
        observation.validate().map_err(|_| 400_u16)?;
        if body.len() > crate::producer::MAX_OBSERVATION_BYTES {
            return Err(400);
        }
        if observation.kind != self.kind.singular() {
            return Err(403);
        }
        let principal = match (&observation.principal, &observation.principal_digest) {
            (Some(principal), None) => principal.clone(),
            (None, Some(digest)) if credential.allow_principal_digest => {
                let mut matches = self
                    .credentials
                    .keys()
                    .filter(|principal| principal_digest(principal) == *digest);
                let principal = matches.next().ok_or(403_u16)?.clone();
                if matches.next().is_some() {
                    return Err(403);
                }
                principal
            }
            _ => return Err(403),
        };
        self.bind(&principal).map_err(|_| 403_u16)?;
        let record = observation.record(principal);
        self.store
            .lock()
            .map_err(|_| 503_u16)?
            .append_produced(record, body)
    }

    fn fetch(&self, principal: &str, path: &str) -> Result<Value, String> {
        let credential = self
            .credentials
            .get(principal)
            .ok_or_else(|| "unknown principal".to_owned())?;
        let (header, value) = self.kind.credential(credential);
        let response = self
            .upstream
            .get_as(path, header, &value)
            .map_err(|_| "upstream unavailable".to_owned())?;
        if response.status != 200 || !response.content_type.starts_with("application/json") {
            return Err("upstream refused request".to_owned());
        }
        let envelope: Value = serde_json::from_slice(&response.body)
            .map_err(|_| "invalid upstream JSON".to_owned())?;
        if envelope.get("ok") != Some(&Value::Bool(true)) {
            return Err("upstream outcome unavailable".to_owned());
        }
        envelope
            .get("result")
            .cloned()
            .ok_or_else(|| "upstream result missing".to_owned())
    }
    fn bind(&self, principal: &str) -> Result<(), String> {
        let identity = self.fetch(principal, "/internal/v1/principal")?;
        let matches = principal_matches(self.kind, &identity, principal);
        if matches {
            Ok(())
        } else {
            Err("credential principal mismatch".to_owned())
        }
    }
    fn ready(&self) -> bool {
        self.upstream
            .get("/readyz")
            .is_ok_and(|response| response.status == 200)
            && self
                .credentials
                .keys()
                .all(|principal| self.bind(principal).is_ok())
            && self
                .store
                .lock()
                .is_ok_and(|store| store.journal.probe_writable().is_ok())
    }
    fn observe(&self, body: &[u8]) -> Result<Record, String> {
        let request: Observe =
            serde_json::from_slice(body).map_err(|_| "invalid observation".to_owned())?;
        if !valid_identifier(&request.resource, 128) {
            return Err("invalid resource".to_owned());
        }
        self.bind(&request.principal)?;
        let snapshot = self.fetch(&request.principal, &self.kind.route(&request.resource))?;
        let record = derive(self.kind, &request.principal, &request.resource, &snapshot)?;
        self.store
            .lock()
            .map_err(|_| "event store unavailable".to_owned())?
            .append(record)
    }
    /// Routes authenticated observations and immutable event reads.
    #[must_use]
    pub fn route(&self, request: &Request) -> Response {
        if request.method == "GET" && request.path == "/livez" {
            return ok("{\"alive\":true}".to_owned());
        }
        if request.method == "GET" && request.path == "/readyz" {
            let ready = self.ready();
            return json(
                if ready { 200 } else { 503 },
                &serde_json::json!({"ready":ready}),
            );
        }
        if request.method == "POST"
            && request.path == "/internal/v1/observe"
            && request.json_body()
            && request.peer_verified
        {
            if let Some(credential) = self
                .producers
                .iter()
                .find(|credential| request.bearer_matches(&credential.token))
            {
                return self
                    .observe_produced(&request.body, credential)
                    .map_or_else(
                        |status| {
                            refusal(status, "observation_refused", (status == 503).then_some(5))
                        },
                        |record| json(200, &record),
                    );
            }
        }
        if !request.peer_verified || !request.bearer_matches(&self.token) {
            return refusal(401, "unauthorized", None);
        }
        if request.method == "POST" && request.path == "/internal/v1/observe" && request.json_body()
        {
            return self.observe(&request.body).map_or_else(
                |_| refusal(503, "source_unavailable", Some(5)),
                |record| json(200, &record),
            );
        }
        if request.method == "GET" {
            if let Some(id) = request
                .path
                .strip_prefix("/internal/v1/events/")
                .filter(|id| valid_hex(id, 32))
            {
                let record = self
                    .store
                    .lock()
                    .ok()
                    .and_then(|store| store.records.get(id).cloned());
                return record.map_or_else(
                    || refusal(404, "event_not_found", None),
                    |record| {
                        if self.bind(&record.principal).is_err() {
                            refusal(503, "source_unavailable", Some(5))
                        } else {
                            json(200, &record)
                        }
                    },
                );
            }
        }
        refusal(404, "not_found", None)
    }
}

fn principal_matches(kind: Kind, identity: &Value, principal: &str) -> bool {
    if matches!(kind, Kind::Journey | Kind::Approval) {
        identity.get("active") == Some(&Value::Bool(true))
            && identity.get("sub").and_then(Value::as_str) == Some(principal)
    } else {
        identity.get("principal_digest").and_then(Value::as_str)
            == Some(principal_digest(principal).as_str())
    }
}

fn principal_digest(principal: &str) -> String {
    sha256_hex(principal.as_bytes())
}

fn derive(kind: Kind, principal: &str, resource: &str, snapshot: &Value) -> Result<Record, String> {
    let committed =
        serde_json::to_vec(&(principal, resource, snapshot)).map_err(|error| error.to_string())?;
    let mut record = Record {
        id: sha256_hex(&committed),
        principal: principal.to_owned(),
        subject: resource.to_owned(),
        subject_sequence: 0,
        occurred_at: unix_seconds()?,
        facts: Vec::new(),
        activity_id: None,
        amount: None,
        asset: None,
    };
    let (identity, fields): (&str, &[&str]) = match kind {
        Kind::Journey => ("journey_id", &["kind", "state", "updated_at"]),
        Kind::Approval => ("approval_id", &["agent_id", "state", "created_at"]),
        Kind::Program => (
            "program_id",
            &["lifecycle", "version", "code_hash", "receipt_digest"],
        ),
        Kind::Payment => ("activity_id", &[]),
    };
    if snapshot.get(identity).and_then(Value::as_str) != Some(resource) {
        return Err("source identity mismatch".to_owned());
    }
    for field in fields {
        let value = snapshot
            .get(*field)
            .ok_or_else(|| "source field missing".to_owned())?;
        record.facts.push(Fact {
            name: (*field).to_owned(),
            value: value
                .as_str()
                .map_or_else(|| value.to_string(), str::to_owned),
        });
    }
    if matches!(kind, Kind::Payment) {
        let bytes = snapshot
            .get("receipt")
            .and_then(Value::as_str)
            .and_then(unhex)
            .ok_or_else(|| "receipt missing".to_owned())?;
        let receipt =
            layerx_wire::receipt::decode(&bytes).map_err(|_| "receipt malformed".to_owned())?;
        let receipt = receipt
            .protocol()
            .ok_or_else(|| "protocol receipt required".to_owned())?;
        if hex(&receipt.activity_id()) != resource {
            return Err("receipt identity mismatch".to_owned());
        }
        record.activity_id = Some(resource.to_owned());
        record.amount = Some(receipt.amount().to_string());
        record.asset = Some(hex(&receipt.asset()));
        record.occurred_at = receipt.timestamp();
    }
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn producer_observation(sequence: u64) -> crate::producer::Observation {
        crate::producer::Observation {
            kind: "journey".to_owned(),
            id: crate::producer::event_id("journey", "journey-one", sequence),
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
    fn producer_retries_require_identical_bytes_after_restart() {
        let directory =
            std::env::temp_dir().join(format!("layerx-produced-events-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        let first = producer_observation(1);
        let bytes = first.encode().unwrap_or_else(|error| panic!("{error}"));
        let record = first.record("principal-one".to_owned());
        let mut store = Store::open(&directory).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            store.append_produced(record.clone(), &bytes),
            Ok(record.clone())
        );
        assert_eq!(
            store.append_produced(record.clone(), &bytes),
            Ok(record.clone())
        );
        assert_eq!(store.journal.len(), 1);
        let mut changed = record.clone();
        changed.facts[0].value = "refused".to_owned();
        let mut changed_body = first.clone();
        changed_body.facts[0].value = "refused".to_owned();
        assert_eq!(
            store.append_produced(
                changed,
                &changed_body
                    .encode()
                    .unwrap_or_else(|error| panic!("{error}"))
            ),
            Err(409)
        );
        let spaced = serde_json::to_vec_pretty(&first).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(store.append_produced(record.clone(), &spaced), Err(409));
        let third = producer_observation(3);
        assert_eq!(
            store.append_produced(
                third.record("principal-one".to_owned()),
                &third.encode().unwrap_or_else(|error| panic!("{error}"))
            ),
            Err(409)
        );
        drop(store);
        let mut store = Store::open(&directory).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(store.append_produced(record.clone(), &bytes), Ok(record));
        assert_eq!(store.journal.len(), 1);
        let second = producer_observation(2);
        assert!(store
            .append_produced(
                second.record("principal-one".to_owned()),
                &second.encode().unwrap_or_else(|error| panic!("{error}"))
            )
            .is_ok());
        assert_eq!(store.journal.len(), 2);
        drop(store);
        std::fs::remove_dir_all(directory).unwrap_or_else(|error| panic!("{error}"));
    }

    #[test]
    fn source_credentials_cannot_be_reassigned_to_a_foreign_principal() {
        let human = serde_json::json!({"active":true,"sub":"principal-one"});
        assert!(principal_matches(Kind::Journey, &human, "principal-one"));
        assert!(!principal_matches(Kind::Approval, &human, "principal-two"));
        assert!(!principal_matches(
            Kind::Journey,
            &serde_json::json!({"active":false,"sub":"principal-one"}),
            "principal-one"
        ));
        let gateway = serde_json::json!({"principal_digest": principal_digest("principal-one")});
        assert!(principal_matches(Kind::Payment, &gateway, "principal-one"));
        assert!(!principal_matches(Kind::Program, &gateway, "principal-two"));
        assert!(!principal_matches(
            Kind::Payment,
            &serde_json::json!({}),
            "principal-one"
        ));
    }

    #[test]
    fn journal_preserves_immutable_event_order_and_deduplicates_observations() {
        let directory = std::env::temp_dir().join(format!("layerx-events-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        let first = derive(Kind::Journey, "principal-one", "journey-one", &serde_json::json!({"journey_id":"journey-one","kind":"move","state":"processing","updated_at":123})).unwrap_or_else(|error| panic!("{error}"));
        let mut store = Store::open(&directory).unwrap_or_else(|error| panic!("{error}"));
        let accepted = store
            .append(first.clone())
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(accepted.subject_sequence, 1);
        assert_eq!(
            store
                .append(first)
                .unwrap_or_else(|error| panic!("{error}"))
                .subject_sequence,
            1
        );
        let second = derive(Kind::Journey, "principal-one", "journey-one", &serde_json::json!({"journey_id":"journey-one","kind":"move","state":"refused","updated_at":124})).unwrap_or_else(|error| panic!("{error}"));
        assert_ne!(accepted.id, second.id);
        assert_eq!(
            store
                .append(second)
                .unwrap_or_else(|error| panic!("{error}"))
                .subject_sequence,
            2
        );
        drop(store);
        let store = Store::open(&directory).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(store.records.len(), 2);
        assert_eq!(store.records[&accepted.id].subject_sequence, 1);
        assert!(derive(
            Kind::Journey,
            "principal-one",
            "foreign",
            &serde_json::json!({"journey_id":"journey-one"})
        )
        .is_err());
        assert!(derive(
            Kind::Payment,
            "principal-one",
            "a",
            &serde_json::json!({"activity_id":"a","receipt":"00"})
        )
        .is_err());
        drop(store);
        std::fs::remove_dir_all(directory).unwrap_or_else(|error| panic!("{error}"));
    }
}
