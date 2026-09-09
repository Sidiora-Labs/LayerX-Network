use std::collections::BTreeMap;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use layerx_human_service::server::movement_provider::{
    MovementProviderCodec, MovementProviderRequest as Request,
    MovementProviderResponse as Response, NativeMovementCodec,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::{hex_string, MAX_FRAME};
use crate::Error;

const MAX_RECORDS: usize = 1024;
const MAX_STATE: usize = 16 * 1024 * 1024;
const MAGIC: &[u8] = b"LXMPJ001";

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    pub request: Vec<u8>,
    pub response: Option<Vec<u8>>,
}

pub(crate) struct Journal {
    root: PathBuf,
    records: BTreeMap<String, Record>,
    healthy: bool,
    protocol: u16,
    _lock: File,
}

pub(crate) fn private_directory(path: &Path) -> Result<(), Error> {
    let meta = fs::symlink_metadata(path)?;
    if !path.is_absolute()
        || fs::canonicalize(path)? != path
        || !meta.is_dir()
        || meta.uid() != rustix::process::geteuid().as_raw()
        || meta.mode() & 0o077 != 0
    {
        return Err(Error::Integrity);
    }
    Ok(())
}

fn nofollow() -> Result<i32, Error> {
    i32::try_from(rustix::fs::OFlags::NOFOLLOW.bits()).map_err(|_| Error::Configuration)
}

pub(crate) fn read_private(path: &Path, maximum: usize) -> Result<Vec<u8>, Error> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(nofollow()?)
        .open(path)?;
    check_file(&file)?;
    if file.metadata()?.len() > maximum as u64 {
        return Err(Error::Capacity);
    }
    let mut bytes = Vec::new();
    file.take(maximum as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err(Error::Capacity);
    }
    Ok(bytes)
}

fn check_file(file: &File) -> Result<(), Error> {
    let meta = file.metadata()?;
    if !meta.is_file()
        || meta.uid() != rustix::process::geteuid().as_raw()
        || meta.mode() & 0o077 != 0
        || meta.nlink() != 1
    {
        return Err(Error::Integrity);
    }
    Ok(())
}

impl Journal {
    pub fn open(root: &Path, protocol: u16) -> Result<Self, Error> {
        NativeMovementCodec::for_protocol(protocol).map_err(|_| Error::Configuration)?;
        if !root.is_absolute() {
            return Err(Error::Configuration);
        }
        match DirBuilder::new().mode(0o700).create(root) {
            Ok(()) => File::open(root.parent().ok_or(Error::Configuration)?)?.sync_all()?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(error) => return Err(error.into()),
        }
        private_directory(root)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(nofollow()?)
            .open(root.join("journal.lock"))?;
        check_file(&lock)?;
        lock.try_lock().map_err(|_| Error::Conflict)?;
        lock.sync_all()?;
        File::open(root)?.sync_all()?;
        let mut value = Self {
            root: root.to_owned(),
            records: BTreeMap::new(),
            healthy: true,
            protocol,
            _lock: lock,
        };
        match read_private(&root.join("journal.bin"), MAX_STATE) {
            Ok(bytes) => value.records = decode(&bytes, protocol)?,
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                value.persist()?;
            }
            Err(error) => return Err(error),
        }
        Ok(value)
    }

    pub fn begin(&mut self, key: &str, request: &[u8]) -> Result<(), Error> {
        if !self.healthy {
            return Err(Error::Integrity);
        }
        validate_key(key)?;
        if let Some(old) = self.records.get(key) {
            return if old.request == request {
                Ok(())
            } else {
                Err(Error::Conflict)
            };
        }
        validate_request(request)?;
        if self.records.len() >= MAX_RECORDS {
            return Err(Error::Capacity);
        }
        self.records.insert(
            key.to_owned(),
            Record {
                request: request.to_vec(),
                response: None,
            },
        );
        self.persist()
    }

    pub fn complete(&mut self, key: &str, response: &[u8]) -> Result<(), Error> {
        if !self.healthy {
            return Err(Error::Integrity);
        }
        let record = self.records.get_mut(key).ok_or(Error::Conflict)?;
        validate_response(&record.request, response, self.protocol)?;
        record.response = Some(response.to_vec());
        self.persist()
    }

    pub fn record(&self, key: &str) -> Option<&Record> {
        self.records.get(key)
    }

    pub fn authorized_plan(
        &self,
        identity: &layerx_human_service::journeys::MovementExecutionIdentity,
    ) -> Result<layerx_human_service::server::movement_provider::PlanningRequest, Error> {
        if !self.healthy {
            return Err(Error::Integrity);
        }
        let codec =
            NativeMovementCodec::for_protocol(self.protocol).map_err(|_| Error::Integrity)?;
        let mut result = None;
        for record in self.records.values() {
            let Some(response) = &record.response else {
                continue;
            };
            let response = codec
                .decode_response(response)
                .map_err(|_| Error::Integrity)?;
            if !matches!(
                response,
                Response::DepositPlan(_) | Response::WithdrawalPlan(_) | Response::ExitPlan(_)
            ) {
                continue;
            }
            let request = codec
                .decode_request(&record.request)
                .map_err(|_| Error::Integrity)?;
            let (Request::PlanDeposit(plan)
            | Request::PlanWithdrawal(plan)
            | Request::PlanExit(plan)) = request
            else {
                return Err(Error::Integrity);
            };
            if plan.principal == identity.principal
                && plan.tenant == identity.tenant
                && plan.idempotency_key == identity.plan_id
            {
                if layerx_paxeer_client::account_address_for_protocol(
                    &plan.context.account,
                    plan.context.protocol_version,
                )
                .map_err(|_| Error::Integrity)?
                    != identity.account
                    || plan.context.wallet != identity.wallet
                    || result.is_some()
                {
                    return Err(Error::Conflict);
                }
                result = Some(plan);
            }
        }
        result.ok_or(Error::Integrity)
    }

    pub fn withdrawal_request(&self, action_key: [u8; 32]) -> Result<Option<Request>, Error> {
        if !self.healthy {
            return Err(Error::Integrity);
        }
        let codec =
            NativeMovementCodec::for_protocol(self.protocol).map_err(|_| Error::Integrity)?;
        let mut found = None;
        for record in self.records.values() {
            let request = codec
                .decode_request(&record.request)
                .map_err(|_| Error::Integrity)?;
            if let Request::SubmitWithdrawal(value) = &request {
                if value.action_key == action_key {
                    if found.is_some() {
                        return Err(Error::Conflict);
                    }
                    found = Some(request);
                }
            }
        }
        Ok(found)
    }

    pub fn has_withdrawal_debit(&self, expected: &layerx_paxeer_client::DebitExpectation) -> bool {
        if !self.healthy {
            return false;
        }
        let Ok(codec) = NativeMovementCodec::for_protocol(self.protocol) else {
            return false;
        };
        self.records.values().any(|record| {
            matches!(codec.decode_request(&record.request), Ok(Request::BindWithdrawalDebit { debit, .. }) if debit == *expected)
                && record.response.as_ref().is_some_and(|bytes| matches!(codec.decode_response(bytes), Ok(Response::Ready)))
        })
    }

    pub fn next_nonce(
        &self,
        wallet: layerx_types::intent::EvmAddress,
        chain: u64,
        observed: u64,
    ) -> Result<u64, Error> {
        if !self.healthy {
            return Err(Error::Integrity);
        }
        let codec =
            NativeMovementCodec::for_protocol(self.protocol).map_err(|_| Error::Integrity)?;
        for record in self.records.values() {
            let Some(bytes) = &record.response else {
                continue;
            };
            let response = codec.decode_response(bytes).map_err(|_| Error::Integrity)?;
            if let Response::PreparedEvmTransaction(transaction) = response {
                let Request::PrepareEvmTransaction { identity, .. } = codec
                    .decode_request(&record.request)
                    .map_err(|_| Error::Integrity)?
                else {
                    return Err(Error::Integrity);
                };
                if identity.wallet == wallet
                    && transaction.chain_id == chain
                    && transaction.nonce >= observed
                {
                    return Err(Error::Conflict);
                }
            }
        }
        Ok(observed)
    }

    fn persist(&mut self) -> Result<(), Error> {
        let result = self.persist_inner();
        if result.is_err() {
            self.healthy = false;
        }
        result
    }

    fn persist_inner(&self) -> Result<(), Error> {
        let body = serde_json::to_vec(&self.records).map_err(|_| Error::Integrity)?;
        let mut bytes = MAGIC.to_vec();
        bytes.extend(self.protocol.to_be_bytes());
        bytes.extend(Sha256::digest(&body));
        bytes.extend(body);
        if bytes.len() > MAX_STATE {
            return Err(Error::Capacity);
        }
        let mut nonce = [0; 16];
        getrandom::fill(&mut nonce).map_err(|_| Error::Integrity)?;
        let temporary = self.root.join(format!("pending-{}", hex_string(&nonce)));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, self.root.join("journal.bin"))?;
        File::open(&self.root)?.sync_all()?;
        Ok(())
    }
}

fn validate_key(key: &str) -> Result<(), Error> {
    if key.len() != 64
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(Error::Integrity);
    }
    Ok(())
}
fn validate_request(bytes: &[u8]) -> Result<(), Error> {
    NativeMovementCodec::new()
        .decode_request(bytes)
        .map_err(|_| Error::Integrity)?;
    Ok(())
}
fn validate_response(request: &[u8], bytes: &[u8], protocol: u16) -> Result<(), Error> {
    if bytes.len() > MAX_FRAME {
        return Err(Error::Capacity);
    }
    let codec = NativeMovementCodec::for_protocol(protocol).map_err(|_| Error::Integrity)?;
    let request = codec
        .decode_request(request)
        .map_err(|_| Error::Integrity)?;
    let response = codec.decode_response(bytes).map_err(|_| Error::Integrity)?;
    if !matches!(
        (request, response),
        (_, Response::Unavailable | Response::ContractViolation)
            | (Request::PlanMove(_), Response::MovePlan(_))
            | (Request::PlanDeposit(_), Response::DepositPlan(_))
            | (Request::PlanWithdrawal(_), Response::WithdrawalPlan(_))
            | (Request::PlanExit(_), Response::ExitPlan(_))
            | (
                Request::VerifyExternalDeposit { .. },
                Response::VerifiedDeposit(_)
            )
            | (
                Request::SubmitDepositCustody(_),
                Response::DepositCustody(_)
            )
            | (
                Request::PollDepositFinality(_),
                Response::DepositFinality(_)
            )
            | (Request::ObtainDepositProof(_), Response::DepositProof(_))
            | (
                Request::VerifyClaimSignature { .. },
                Response::ClaimTransaction(_)
            )
            | (
                Request::BindWithdrawalDebit { .. } | Request::Readiness,
                Response::Ready
            )
            | (Request::CheckpointProof(_), Response::CheckpointProof(_))
            | (Request::SubmitWithdrawal(_), Response::Withdrawal(_))
            | (Request::LookupWithdrawal(_), Response::WithdrawalLookup(_))
            | (Request::SubmitExit(_), Response::Exit(_))
            | (
                Request::PrepareEvmTransaction { .. },
                Response::PreparedEvmTransaction(_)
            )
    ) {
        return Err(Error::Integrity);
    }
    Ok(())
}
fn decode(bytes: &[u8], protocol: u16) -> Result<BTreeMap<String, Record>, Error> {
    if bytes.len() < 42
        || bytes.len() > MAX_STATE
        || &bytes[..8] != MAGIC
        || bytes[8..10] != protocol.to_be_bytes()
    {
        return Err(Error::Integrity);
    }
    let body = &bytes[42..];
    if Sha256::digest(body).as_slice() != &bytes[10..42] {
        return Err(Error::Integrity);
    }
    let records: BTreeMap<String, Record> =
        serde_json::from_slice(body).map_err(|_| Error::Integrity)?;
    if records.len() > MAX_RECORDS
        || serde_json::to_vec(&records).map_err(|_| Error::Integrity)? != body
    {
        return Err(Error::Integrity);
    }
    for (key, record) in &records {
        validate_key(key)?;
        validate_request(&record.request)?;
        if let Some(response) = &record.response {
            validate_response(&record.request, response, protocol)?;
        }
    }
    Ok(records)
}

pub(crate) fn socket_lock(path: &Path) -> Result<File, Error> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(nofollow()?)
        .open(path)?;
    check_file(&file)?;
    file.try_lock().map_err(|_| Error::Conflict)?;
    Ok(file)
}
