use layerx_paxeer_client::{raw_call, EndpointConfig, Json, TrackerConfig, ANCHOR_PRECOMPILE};

use crate::config::{hex, hex_string};
use crate::Error;

/// `finalizedStateRoot(uint64)` on the anchor precompile.
const SELECTOR_FINALIZED_STATE_ROOT: [u8; 4] = [0x0f, 0x60, 0x7f, 0xe4];
/// `finalizedReceiptRoot(uint64)` on the anchor precompile.
const SELECTOR_FINALIZED_RECEIPT_ROOT: [u8; 4] = [0xe0, 0xa3, 0xcc, 0xaa];

/// Confirms, with the configured endpoint agreement, that the anchor
/// precompile reports `batch_number` finalized with exactly the state and
/// receipt roots the sequencer-signed batch header commits to.
///
/// This replaces the registry-publication check the Solidity checkpoint
/// registry used to answer: finalized roots are now native chain state served
/// by `0x…1014`, not an event a registry contract published.
pub(crate) fn verify_finalised_batch(
    tracker: &TrackerConfig,
    batch_number: u64,
    state_root: [u8; 32],
    receipt_root: [u8; 32],
) -> Result<(), Error> {
    if batch_number == 0 || state_root == [0; 32] || receipt_root == [0; 32] {
        return Err(Error::Integrity);
    }
    let mut agreement = 0;
    for endpoint in &tracker.endpoints {
        if matches!(
            finalised_root(endpoint, SELECTOR_FINALIZED_STATE_ROOT, batch_number),
            Ok(Some(observed)) if observed == state_root
        ) && matches!(
            finalised_root(endpoint, SELECTOR_FINALIZED_RECEIPT_ROOT, batch_number),
            Ok(Some(observed)) if observed == receipt_root
        ) {
            agreement += 1;
        }
    }
    if agreement < tracker.minimum_endpoint_agreement {
        return Err(Error::Integrity);
    }
    Ok(())
}

/// Reads one anchored root for `batch_number` from a single origin. `None`
/// means the origin reports the batch is not finalized yet.
fn finalised_root(
    endpoint: &EndpointConfig,
    selector: [u8; 4],
    batch_number: u64,
) -> Result<Option<[u8; 32]>, Error> {
    let mut data = selector.to_vec();
    data.extend([0; 24]);
    data.extend(batch_number.to_be_bytes());
    let call = Json::Object(vec![
        ("to".to_owned(), text_hex(&ANCHOR_PRECOMPILE.bytes())),
        ("data".to_owned(), text_hex(&data)),
    ]);
    let answer = raw_call(
        endpoint,
        "eth_call",
        &[call, Json::Text("latest".to_owned())],
    )
    .map_err(|_| Error::Integrity)?;
    decode_finalised_root(answer.as_text().ok_or(Error::Integrity)?)
}

/// Decodes the `(bytes32 root, bool finalized)` answer of an anchor root view.
///
/// Anything but the two declared canonical words is refused: a short or long
/// return, a boolean word outside `{0, 1}` and a root claimed for a batch the
/// anchor does not report finalized.
fn decode_finalised_root(answer: &str) -> Result<Option<[u8; 32]>, Error> {
    let words = hex::<64>(answer).map_err(|_| Error::Integrity)?;
    let root: [u8; 32] = words[..32].try_into().map_err(|_| Error::Integrity)?;
    if words[32..63] != [0; 31] {
        return Err(Error::Integrity);
    }
    match words[63] {
        0 if root == [0; 32] => Ok(None),
        1 if root != [0; 32] => Ok(Some(root)),
        _ => Err(Error::Integrity),
    }
}

fn text_hex(bytes: &[u8]) -> Json {
    Json::Text(format!("0x{}", hex_string(bytes)))
}

#[cfg(test)]
mod tests {
    use super::{decode_finalised_root, verify_finalised_batch};

    /// The exact `cast abi-encode "f(bytes32,bool)" 0x2222…22 true` answer of a
    /// finalized batch, and the `0x00…00 false` answer of one that is not.
    const FINALIZED: &str = concat!(
        "0x2222222222222222222222222222222222222222222222222222222222222222",
        "0000000000000000000000000000000000000000000000000000000000000001"
    );
    const NOT_FINALIZED: &str = concat!(
        "0x0000000000000000000000000000000000000000000000000000000000000000",
        "0000000000000000000000000000000000000000000000000000000000000000"
    );

    #[test]
    fn anchor_answers_are_only_accepted_in_the_declared_two_word_abi_shape(
    ) -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(decode_finalised_root(FINALIZED)?, Some([0x22; 32]));
        assert_eq!(decode_finalised_root(NOT_FINALIZED)?, None);
        let refusals = vec![
            // One word only: the boolean is missing.
            FINALIZED[..66].to_owned(),
            // Three words: the anchor view returns exactly two.
            format!("{FINALIZED}{}", "00".repeat(32)),
            // A boolean word outside {0, 1}.
            format!("0x{}{}02", "22".repeat(32), "00".repeat(31)),
            // A non-canonical boolean carrying high bits.
            format!("0x{}01{}01", "22".repeat(32), "00".repeat(30)),
            // A root claimed while the anchor reports the batch unfinalized.
            format!("0x{}{}", "22".repeat(32), "00".repeat(32)),
            // The finalized flag without a root.
            format!("0x{}{}01", "00".repeat(32), "00".repeat(31)),
            // Not hexadecimal at all.
            "0xzz".to_owned(),
            String::new(),
        ];
        for refused in &refusals {
            assert!(
                decode_finalised_root(refused).is_err(),
                "accepted a malformed anchor answer: {refused}"
            );
        }
        Ok(())
    }

    #[test]
    fn an_origin_that_does_not_anchor_the_batch_never_reaches_agreement(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = crate::tests::Directory::new()?;
        let config = crate::tests::config(&dir)?;
        assert!(verify_finalised_batch(&config.tracker, 2, [0x22; 32], [0x33; 32]).is_err());
        assert!(verify_finalised_batch(&config.tracker, 0, [0x22; 32], [0x33; 32]).is_err());
        assert!(verify_finalised_batch(&config.tracker, 2, [0; 32], [0x33; 32]).is_err());
        assert!(verify_finalised_batch(&config.tracker, 2, [0x22; 32], [0; 32]).is_err());
        Ok(())
    }
}
