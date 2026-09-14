use std::error::Error;
use std::path::Path;
use std::time::Duration;

use layerx_client::availability::{
    self, AvailabilitySelector, FetchContext, FetchOutcome, Provider, ProviderSet, RetrievalLimits,
};
use layerx_client::handover::SequencerHistory;
use layerx_client::lni::handshake::{perform, HandshakeConfig};
use layerx_client::lni::schema::Version;
use layerx_client::lni::transport::{ConnectionGate, Limits, Uds};
use layerx_proof::availability::{AvailabilityClass, RootCommitments};
use layerx_proof::inclusion::verify_header;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_wire::handover::{decode_certificate, decode_evidence, decode_recovery};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

struct Reader<'a>(&'a [u8]);
impl Reader<'_> {
    fn take(&mut self, count: usize) -> Result<&[u8]> {
        let value = self.0.get(..count).ok_or("truncated public export")?;
        self.0 = &self.0[count..];
        Ok(value)
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into()?))
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.take(N)?.try_into()?)
    }
    fn span(&mut self, maximum: usize) -> Result<Vec<u8>> {
        let count = usize::try_from(self.u32()?)?;
        if count > maximum {
            return Err("public export bound".into());
        }
        Ok(self.take(count)?.to_vec())
    }
}

struct Genesis {
    network: u32,
    root: [u8; 32],
    key: [u8; 32],
    witness: Vec<u8>,
    registry: ModuleRegistry,
}

impl Genesis {
    fn read(directory: &Path) -> Result<Self> {
        let bytes = std::fs::read(directory.join("handover-genesis.bin"))?;
        let mut reader = Reader(&bytes);
        let network = reader.u32()?;
        let root = reader.array()?;
        let key = reader.array()?;
        let witness = reader.span(1_048_576)?;
        let count = reader.u32()?;
        if count > 9 {
            return Err("module count".into());
        }
        let mut modules = Vec::new();
        for _ in 0..count {
            let module = ModuleId::from_u16(u16::try_from(reader.u32()?)?)
                .map_err(|error| format!("{error:?}"))?;
            let kinds = reader.u32()?;
            if kinds > 64 {
                return Err("activity count".into());
            }
            let mut activities = Vec::new();
            for _ in 0..kinds {
                activities.push(
                    ActivityType::from_u32(reader.u32()?).map_err(|error| format!("{error:?}"))?,
                );
            }
            modules.push(
                ModuleRegistration::new(module, &activities)
                    .map_err(|error| format!("{error:?}"))?,
            );
        }
        if !reader.0.is_empty() {
            return Err("trailing genesis export".into());
        }
        Ok(Self {
            network,
            root,
            key,
            witness,
            registry: ModuleRegistry::new(&modules).map_err(|error| format!("{error:?}"))?,
        })
    }

    fn history(&self) -> Result<SequencerHistory> {
        SequencerHistory::from_genesis(
            self.network,
            self.root,
            self.key,
            &self.witness,
            self.registry.clone(),
        )
        .map_err(|error| format!("{error:?}").into())
    }

    fn reject_substitutions(&self) {
        let mut root = self.root;
        root[0] ^= 1;
        assert!(SequencerHistory::from_genesis(
            self.network,
            root,
            self.key,
            &self.witness,
            self.registry.clone()
        )
        .is_err());
        let mut witness = self.witness.clone();
        witness[4] ^= 1;
        assert!(SequencerHistory::from_genesis(
            self.network,
            self.root,
            self.key,
            &witness,
            self.registry.clone()
        )
        .is_err());
    }
}

fn connect(socket: &Path, network: u32) -> Result<Uds> {
    let mut transport = Uds::connect(
        socket,
        &ConnectionGate::new(1),
        Limits {
            maximum_frame_bytes: 16_777_216,
            maximum_connections: 1,
            maximum_streams: 1,
            maximum_queued_bytes: 16_777_216,
            deadline: Duration::from_secs(8),
        },
    )
    .map_err(|error| format!("{error:?}"))?;
    let handshake = perform(
        &mut transport,
        &HandshakeConfig {
            built_interface_version: Version::V1_5,
            expected_protocol_version: 3,
            expected_network_id: network,
        },
        None,
    )
    .map_err(|error| format!("{error:?}"))?;
    assert_eq!(handshake.node().interface_version, Version::V1_5);
    Ok(transport)
}

fn limits() -> RetrievalLimits {
    RetrievalLimits {
        maximum_bytes: 16_777_216,
        maximum_chunks: 4096,
        deadline: Duration::from_secs(8),
    }
}

fn reject_missing_material(
    socket: &Path,
    network: u32,
    history: &SequencerHistory,
    batch: u64,
) -> Result<()> {
    let mut transport = connect(socket, network)?;
    let candidate = layerx_client::batch::lookup_untrusted(&mut transport, Version::V1_5, batch, 1)
        .map_err(|error| format!("{error:?}"))?;
    let header = candidate.header();
    let result = availability::fetch(
        &mut ProviderSet::new(vec![Provider {
            name: "native".into(),
            transport: &mut transport,
        }]),
        AvailabilitySelector::Batch(batch),
        FetchContext {
            interface_version: Version::V1_5,
            correlation_id: 2,
            expected_batch_number: batch,
            data_availability_root: header.data_availability_root(),
            record_roots: RootCommitments {
                activity: header.activity_merkle_root(),
                receipt: header.receipt_merkle_root(),
                event: header.event_merkle_root(),
                oracle: header.oracle_root(),
            },
            limits: limits(),
        },
        |_| {},
    )
    .map_err(|error| format!("{error:?}"))?;
    let FetchOutcome::Complete(mut availability) = result else {
        return Err("incomplete actual availability".into());
    };
    let mut attempted = history.clone();
    let prior_epoch = history
        .verified_head()
        .map_or(1, |prior| prior.header().epoch());
    if header.epoch() != prior_epoch {
        assert!(attempted
            .advance(
                candidate.canonical_bytes(),
                candidate.signature(),
                &availability,
                None
            )
            .is_err());
        assert_eq!(&attempted, history);
        let mut recovery = Vec::new();
        for chunk in &availability.chunks {
            if chunk.chunk().class == AvailabilityClass::Recovery {
                recovery.extend_from_slice(&chunk.chunk().bytes);
            }
        }
        let (_, packet) = decode_recovery(&recovery).map_err(|error| format!("{error:?}"))?;
        let packet = packet.ok_or("missing genuine handover packet")?;
        let evidence = decode_evidence(packet).map_err(|error| format!("{error:?}"))?;
        for end in [0, 1, 31, 392, packet.len() - 1] {
            assert!(decode_evidence(&packet[..end]).is_err());
        }
        for end in [0, 1, 327] {
            assert!(decode_certificate(&evidence.signed_certificate[..end]).is_err());
        }
        let mut altered = packet.to_vec();
        let last = altered.len() - 1;
        altered[last] ^= 1;
        assert!(decode_evidence(&altered).is_err());
    }
    availability
        .chunks
        .retain(|chunk| chunk.chunk().class != AvailabilityClass::Recovery);
    assert!(attempted
        .advance(
            candidate.canonical_bytes(),
            candidate.signature(),
            &availability,
            None
        )
        .is_err());
    assert_eq!(&attempted, history);
    Ok(())
}

fn reject_signed_forgeries(directory: &Path, history: &SequencerHistory, count: u64) -> Result<()> {
    let mut retired = 0;
    let mut unauthorized = 0;
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_str().ok_or("export filename")?;
        if !name.starts_with("retired-") && !name.starts_with("unauthorized-") {
            continue;
        }
        let bytes = std::fs::read(entry.path())?;
        let mut reader = Reader(&bytes);
        let canonical = reader.span(354)?;
        let signature = reader.array()?;
        assert!(reader.0.is_empty());
        let header = layerx_wire::receipt::decode_batch_header(&canonical)
            .map_err(|error| format!("{error:?}"))?;
        let authorization = history
            .authorization_for_batch(header.batch_number())
            .map_err(|error| format!("{error:?}"))?;
        assert!(verify_header(&canonical, &signature, &authorization).is_err());
        if name.starts_with("retired-") {
            retired += 1;
        } else {
            unauthorized += 1;
        }
    }
    assert_eq!(unauthorized, count);
    assert_eq!(retired, 2);
    assert!(history.authorization_for_batch(count + 1).is_err());
    let head = history.verified_head().ok_or("unverified head")?;
    assert!(history
        .authorization_for_sequence(head.header().last_sequence() + 1)
        .is_err());
    assert_ne!(
        history.authorization_for_batch(1),
        history.authorization_for_batch(count)
    );
    Ok(())
}

fn main() -> Result<()> {
    let arguments: Vec<String> = std::env::args().collect();
    if arguments.len() != 4 {
        return Err(
            "usage: native_handover_history SOCKET PUBLIC_EXPORT_DIRECTORY BATCH_COUNT".into(),
        );
    }
    let socket = Path::new(&arguments[1]);
    let directory = Path::new(&arguments[2]);
    let count: u64 = arguments[3].parse()?;
    if count < 3 || count > 512 {
        return Err("bounded native history required".into());
    }
    let genesis = Genesis::read(directory)?;
    genesis.reject_substitutions();
    let mut expected = None;
    for replay in 0..2 {
        let mut history = genesis.history()?;
        assert!(history.authorization_for_batch(1).is_err());
        for batch in 1..=count {
            if replay == 0 {
                reject_missing_material(socket, genesis.network, &history, batch)?;
            }
            let mut transport = connect(socket, genesis.network)?;
            history
                .fetch_next(&mut transport, Version::V1_5, 1, limits())
                .map_err(|error| format!("native history batch {batch}: {error:?}"))?;
            assert_eq!(
                history
                    .verified_head()
                    .ok_or("head")?
                    .header()
                    .batch_number(),
                batch
            );
        }
        reject_signed_forgeries(directory, &history, count)?;
        if let Some(previous) = &expected {
            assert_eq!(&history, previous);
        }
        expected = Some(history);
    }
    println!("native genesis-bound public history verified {count} batches twice; forged and retired keys, missing finality, missing recovery, substituted genesis and future authority refused");
    Ok(())
}
