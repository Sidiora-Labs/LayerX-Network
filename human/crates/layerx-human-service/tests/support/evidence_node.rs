use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::thread::{self, JoinHandle};

use layerx_client::lni::framing::{read_frame, write_frame};
use layerx_client::lni::handshake::{encode_node_info, NodeInfo, NodeRole};
use layerx_client::lni::schema::{
    decode_envelope, encode_envelope, Capability, Envelope, SchemaError, Version,
};
use layerx_crypto::{ed25519, SignatureMessage};
use layerx_types::payload::ModuleRegistry;
use layerx_types::result::{KnownResult, ResultCode};
use layerx_wire::activity::{decode_signed, encode_unsigned};
use layerx_wire::hash::{activity_id, payload_hash, Domain};
use layerx_wire::limits::{MAX_MESSAGE_BYTES, PROTOCOL_VERSION};
use sha2::{Digest as _, Sha256};

const NODE_INFO_REQUEST_TAG: u16 = 1;
const NODE_INFO_RESPONSE_TAG: u16 = 2;
const SUBMIT_REQUEST_TAG: u16 = 3;
const SUBMIT_RESPONSE_TAG: u16 = 4;
const ERROR_RESPONSE_TAG: u16 = 25;

const MALFORMED_REFUSAL_CLASS: u8 = 1;
const ADMISSION_REFUSAL_CLASS: u8 = 4;
const AUTHENTICATION_REFUSAL_CLASS: u8 = 6;

const JOURNAL_NAME: &str = "admission.journal";
const JOURNAL_MAGIC: &[u8; 4] = b"LXHA";
const JOURNAL_VERSION: u16 = 1;
const JOURNAL_SUPERBLOCK_BYTES: usize = 10;
const RECORD_MAGIC: &[u8; 4] = b"LXAR";
const RECORD_HEADER_BYTES: usize = 40;
const RECORD_DIGEST_BYTES: usize = 32;

pub const FRAME_BYTES: usize = 1_048_576;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Refusal {
    pub class: u8,
    pub result: ResultCode,
}

impl Refusal {
    const fn known(class: u8, result: KnownResult) -> Self {
        Self {
            class,
            result: ResultCode::from_raw(result.raw()),
        }
    }

    fn payload(self) -> [u8; 5] {
        let mut payload = [0_u8; 5];
        payload[0] = self.class;
        payload[1..].copy_from_slice(&self.result.raw().to_be_bytes());
        payload
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdmissionRecord {
    pub activity_id: [u8; 32],
    pub activity: Vec<u8>,
}

#[derive(Debug)]
pub struct AdmissionJournal {
    directory: PathBuf,
    file: File,
    records: Vec<AdmissionRecord>,
    admitted: BTreeSet<[u8; 32]>,
}

impl AdmissionJournal {
    pub fn open(directory: &Path, network_id: u32) -> Self {
        std::fs::create_dir_all(directory)
            .unwrap_or_else(|error| panic!("admission directory: {error}"));
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
            .unwrap_or_else(|error| panic!("admission directory permissions: {error}"));
        let path = directory.join(JOURNAL_NAME);
        let existed = path.exists();
        let mut file = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .mode(0o600)
            .open(&path)
            .unwrap_or_else(|error| panic!("admission journal: {error}"));
        let records = if existed {
            Self::recover_from(&mut file, network_id)
        } else {
            file.write_all(&superblock(network_id))
                .unwrap_or_else(|error| panic!("admission superblock: {error}"));
            file.sync_data()
                .unwrap_or_else(|error| panic!("admission superblock fdatasync: {error}"));
            File::open(directory)
                .and_then(|parent| parent.sync_all())
                .unwrap_or_else(|error| panic!("admission directory fsync: {error}"));
            Vec::new()
        };
        let admitted = records.iter().map(|record| record.activity_id).collect();
        Self {
            directory: directory.to_path_buf(),
            file,
            records,
            admitted,
        }
    }

    pub fn recover(directory: &Path, network_id: u32) -> Vec<AdmissionRecord> {
        let mut file = File::open(directory.join(JOURNAL_NAME))
            .unwrap_or_else(|error| panic!("recover admission journal: {error}"));
        Self::recover_from(&mut file, network_id)
    }

    fn recover_from(file: &mut File, network_id: u32) -> Vec<AdmissionRecord> {
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .unwrap_or_else(|error| panic!("read admission journal: {error}"));
        assert!(
            bytes.len() >= JOURNAL_SUPERBLOCK_BYTES
                && bytes[..JOURNAL_SUPERBLOCK_BYTES] == superblock(network_id),
            "admission journal superblock mismatch"
        );
        let mut records = Vec::new();
        let mut cursor = JOURNAL_SUPERBLOCK_BYTES;
        while cursor + RECORD_HEADER_BYTES <= bytes.len() {
            let header = &bytes[cursor..cursor + RECORD_HEADER_BYTES];
            if &header[..4] != RECORD_MAGIC {
                break;
            }
            let length = usize::try_from(u32::from_be_bytes([
                header[4], header[5], header[6], header[7],
            ]))
            .unwrap_or_else(|_| panic!("admission record length overflow"));
            let mut activity_id = [0_u8; 32];
            activity_id.copy_from_slice(&header[8..40]);
            let body_end = cursor + RECORD_HEADER_BYTES + length;
            let record_end = body_end + RECORD_DIGEST_BYTES;
            if length == 0 || length > MAX_MESSAGE_BYTES || record_end > bytes.len() {
                break;
            }
            let activity = &bytes[cursor + RECORD_HEADER_BYTES..body_end];
            if bytes[body_end..record_end] != record_digest(&header[4..40], activity) {
                break;
            }
            records.push(AdmissionRecord {
                activity_id,
                activity: activity.to_vec(),
            });
            cursor = record_end;
        }
        records
    }

    #[must_use]
    pub fn contains(&self, activity_id: &[u8; 32]) -> bool {
        self.admitted.contains(activity_id)
    }

    #[must_use]
    pub fn records(&self) -> &[AdmissionRecord] {
        &self.records
    }

    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    fn admit(&mut self, activity_id: [u8; 32], activity: &[u8]) -> std::io::Result<()> {
        let mut record =
            Vec::with_capacity(RECORD_HEADER_BYTES + activity.len() + RECORD_DIGEST_BYTES);
        record.extend_from_slice(RECORD_MAGIC);
        let length = u32::try_from(activity.len())
            .map_err(|_| std::io::Error::other("admission record length overflow"))?;
        record.extend_from_slice(&length.to_be_bytes());
        record.extend_from_slice(&activity_id);
        record.extend_from_slice(activity);
        let digest = record_digest(&record[4..RECORD_HEADER_BYTES], activity);
        record.extend_from_slice(&digest);
        self.file.write_all(&record)?;
        self.file.sync_data()?;
        self.records.push(AdmissionRecord {
            activity_id,
            activity: activity.to_vec(),
        });
        self.admitted.insert(activity_id);
        Ok(())
    }
}

fn superblock(network_id: u32) -> [u8; JOURNAL_SUPERBLOCK_BYTES] {
    let mut bytes = [0_u8; JOURNAL_SUPERBLOCK_BYTES];
    bytes[..4].copy_from_slice(JOURNAL_MAGIC);
    bytes[4..6].copy_from_slice(&JOURNAL_VERSION.to_be_bytes());
    bytes[6..10].copy_from_slice(&network_id.to_be_bytes());
    bytes
}

fn record_digest(header_tail: &[u8], activity: &[u8]) -> [u8; RECORD_DIGEST_BYTES] {
    let mut digest = Sha256::new();
    digest.update(b"LXH/v1/admission-record\0");
    digest.update(header_tail);
    digest.update(activity);
    digest.finalize().into()
}

#[derive(Debug)]
pub struct EvidenceNode {
    info: NodeInfo,
    registry: ModuleRegistry,
    identities: BTreeMap<Vec<u8>, [u8; 32]>,
    journal: AdmissionJournal,
    queue: VecDeque<[u8; 32]>,
    queue_capacity: usize,
    authentication_refusals: u64,
    fail_stopped: bool,
}

impl EvidenceNode {
    pub fn new(
        authorised_sequencer_key: [u8; 32],
        network_id: u32,
        registry: ModuleRegistry,
        admission_directory: &Path,
        queue_capacity: usize,
    ) -> Self {
        assert!(
            queue_capacity > 0,
            "admission queue capacity must be bounded above zero"
        );
        Self {
            info: NodeInfo {
                interface_version: Version::V1_3,
                protocol_version: PROTOCOL_VERSION,
                network_id,
                role: NodeRole::Sequencer,
                chain_head_sequence: 50,
                latest_sealed_batch: 7,
                latest_finalised_checkpoint: [0x91; 32],
                authorised_sequencer_key,
                advertised_capabilities: vec![
                    Capability::AuthenticatedDurableSubmit.name().to_owned(),
                    Capability::NodeInfo.name().to_owned(),
                    Capability::Submit.name().to_owned(),
                ],
            },
            registry,
            identities: BTreeMap::new(),
            journal: AdmissionJournal::open(admission_directory, network_id),
            queue: VecDeque::new(),
            queue_capacity,
            authentication_refusals: 0,
            fail_stopped: false,
        }
    }

    pub fn register_identity(&mut self, actor_did: &[u8], owner_key: [u8; 32]) {
        self.identities.insert(actor_did.to_vec(), owner_key);
    }

    #[must_use]
    pub const fn journal(&self) -> &AdmissionJournal {
        &self.journal
    }

    #[must_use]
    pub fn queued(&self) -> Vec<[u8; 32]> {
        self.queue.iter().copied().collect()
    }

    #[must_use]
    pub const fn authentication_refusals(&self) -> u64 {
        self.authentication_refusals
    }

    #[must_use]
    pub const fn fail_stopped(&self) -> bool {
        self.fail_stopped
    }

    pub fn serve(mut self, listener: UnixListener) -> JoinHandle<Self> {
        thread::spawn(move || {
            let (mut stream, _) = listener
                .accept()
                .unwrap_or_else(|error| panic!("accept evidence node connection: {error}"));
            self.serve_connection(&mut stream);
            self
        })
    }

    fn serve_connection(&mut self, stream: &mut UnixStream) {
        let request = read_frame(stream, FRAME_BYTES)
            .unwrap_or_else(|error| panic!("read evidence handshake: {error:?}"));
        let request = decode_envelope(&request)
            .unwrap_or_else(|error| panic!("decode evidence handshake: {error:?}"));
        assert_eq!(request.message_tag, NODE_INFO_REQUEST_TAG);
        assert_eq!(request.correlation_id, 0);
        assert!(request.canonical_payload.is_empty());
        assert!(request.proof_material.is_empty());
        let payload = encode_node_info(&self.info)
            .unwrap_or_else(|error| panic!("encode evidence node information: {error:?}"));
        self.respond(stream, NODE_INFO_RESPONSE_TAG, 0, &payload, &[]);
        while !self.fail_stopped {
            let Ok(frame) = read_frame(stream, FRAME_BYTES) else {
                break;
            };
            let Ok(request) = decode_envelope(&frame) else {
                break;
            };
            let correlation_id = request.correlation_id;
            if request.message_tag != SUBMIT_REQUEST_TAG {
                let refusal =
                    Refusal::known(MALFORMED_REFUSAL_CLASS, KnownResult::MalformedEnvelope);
                self.respond(
                    stream,
                    ERROR_RESPONSE_TAG,
                    correlation_id,
                    &refusal.payload(),
                    &[],
                );
                continue;
            }
            match self.admit(&request) {
                Ok(activity_id) => {
                    let echoed = request.canonical_payload.to_vec();
                    self.respond(
                        stream,
                        SUBMIT_RESPONSE_TAG,
                        correlation_id,
                        &echoed,
                        &activity_id,
                    );
                }
                Err(Some(refusal)) => {
                    self.respond(
                        stream,
                        ERROR_RESPONSE_TAG,
                        correlation_id,
                        &refusal.payload(),
                        &[],
                    );
                }
                Err(None) => {
                    self.fail_stopped = true;
                }
            }
        }
    }

    fn respond(
        &self,
        stream: &mut UnixStream,
        message_tag: u16,
        correlation_id: u64,
        canonical_payload: &[u8],
        proof_material: &[u8],
    ) {
        let response = encode_envelope(Envelope {
            version: self.info.interface_version,
            message_tag,
            correlation_id,
            canonical_payload,
            proof_material,
        })
        .unwrap_or_else(|error: SchemaError| panic!("encode evidence node response: {error:?}"));
        write_frame(stream, &response, FRAME_BYTES)
            .unwrap_or_else(|error| panic!("write evidence node response: {error:?}"));
    }

    fn admit(&mut self, request: &Envelope<'_>) -> Result<[u8; 32], Option<Refusal>> {
        let payload = request.canonical_payload;
        if !request.proof_material.is_empty()
            || payload.is_empty()
            || payload.len() > MAX_MESSAGE_BYTES
        {
            return Err(Some(Refusal::known(
                MALFORMED_REFUSAL_CLASS,
                KnownResult::MalformedEnvelope,
            )));
        }
        let activity = decode_signed(payload, &self.registry).map_err(|error| {
            Some(Refusal {
                class: ADMISSION_REFUSAL_CLASS,
                result: error.result,
            })
        })?;
        if activity.protocol_version() != self.info.protocol_version {
            return Err(Some(Refusal::known(
                ADMISSION_REFUSAL_CLASS,
                KnownResult::VersionUnsupported,
            )));
        }
        if activity.network_id() != self.info.network_id {
            return Err(Some(Refusal::known(
                ADMISSION_REFUSAL_CLASS,
                KnownResult::WrongNetwork,
            )));
        }
        let expected_payload_hash = payload_hash(&activity).map_err(|error| {
            Some(Refusal {
                class: ADMISSION_REFUSAL_CLASS,
                result: error.result,
            })
        })?;
        if expected_payload_hash != activity.payload_hash() {
            return Err(Some(Refusal::known(
                ADMISSION_REFUSAL_CLASS,
                KnownResult::PayloadHashMismatch,
            )));
        }
        let identifier = activity_id(&activity).map_err(|error| {
            Some(Refusal {
                class: ADMISSION_REFUSAL_CLASS,
                result: error.result,
            })
        })?;

        let Some(current_key) = self.identities.get(activity.actor_did()).copied() else {
            return Err(Some(self.authentication_refusal(KnownResult::UnknownDid)));
        };
        if activity.authority() != current_key.as_slice() {
            return Err(Some(self.authentication_refusal(KnownResult::BadSignature)));
        }
        let Some(signature) = activity
            .signature()
            .and_then(|bytes| <[u8; 64]>::try_from(bytes).ok())
        else {
            return Err(Some(self.authentication_refusal(KnownResult::BadSignature)));
        };
        let unsigned = encode_unsigned(&activity).map_err(|error| {
            Some(Refusal {
                class: ADMISSION_REFUSAL_CLASS,
                result: error.result,
            })
        })?;
        let message = SignatureMessage::new(
            Domain::SignaturePreimage,
            activity.protocol_version(),
            activity.network_id(),
            &unsigned,
        )
        .map_err(|error| Some(self.authentication_refusal_code(error.result_code())))?;
        if ed25519::verify(&current_key, &signature, message).is_err() {
            return Err(Some(self.authentication_refusal(KnownResult::BadSignature)));
        }

        if self.journal.contains(&identifier) {
            return Ok(identifier);
        }
        if self.queue.len() >= self.queue_capacity {
            return Err(Some(Refusal::known(
                ADMISSION_REFUSAL_CLASS,
                KnownResult::LengthLimit,
            )));
        }
        self.journal.admit(identifier, payload).map_err(|_| None)?;
        self.queue.push_back(identifier);
        Ok(identifier)
    }

    fn authentication_refusal(&mut self, result: KnownResult) -> Refusal {
        self.authentication_refusal_code(ResultCode::from_raw(result.raw()))
    }

    fn authentication_refusal_code(&mut self, result: ResultCode) -> Refusal {
        self.authentication_refusals = self.authentication_refusals.saturating_add(1);
        Refusal {
            class: AUTHENTICATION_REFUSAL_CLASS,
            result,
        }
    }
}
