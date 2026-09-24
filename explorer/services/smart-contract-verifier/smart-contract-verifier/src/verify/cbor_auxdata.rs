use super::Error;
use crate::Language;
use mismatch::Mismatch;
use verification_common::verifier_alliance::{CborAuxdata, CborAuxdataValue};

pub fn retrieve_cbor_auxdata(
    language: Language,
    code: &[u8],
    modified_code: &[u8],
) -> Result<Option<CborAuxdata>, Error> {
    if code.len() != modified_code.len() {
        Err(anyhow::anyhow!(
            "bytecode and modified bytecode length mismatch: {}",
            Mismatch::new(code.len(), modified_code.len())
        ))?
    }

    let mut cbor_auxdata = CborAuxdata::new();
    match language {
        Language::Solidity | Language::Yul => add_cbor_auxadata_recursive::<SolcCborAuxdata>(
            &mut cbor_auxdata,
            code,
            modified_code,
            0,
        )?,
        Language::Vyper => add_cbor_auxadata_recursive::<VyperCborAuxdata>(
            &mut cbor_auxdata,
            code,
            modified_code,
            0,
        )?,
    }

    Ok(Some(cbor_auxdata))
}

trait CborAuxdataType {
    fn from_cbor(encoded: &[u8]) -> Result<usize, ()>;

    fn validate_encoded_length(encoded_length: usize, actual_size: usize) -> bool;
}

struct SolcCborAuxdata;
impl CborAuxdataType for SolcCborAuxdata {
    fn from_cbor(encoded: &[u8]) -> Result<usize, ()> {
        solidity_metadata::MetadataHash::from_cbor(encoded)
            .map(|(_, size)| size)
            .map_err(|_| ())
    }

    fn validate_encoded_length(encoded_length: usize, actual_size: usize) -> bool {
        // Solidity does not count 2 length related bytes as part of the auxdata
        encoded_length == actual_size
    }
}

struct VyperCborAuxdata;
impl CborAuxdataType for VyperCborAuxdata {
    fn from_cbor(encoded: &[u8]) -> Result<usize, ()> {
        vyper_cbor_auxdata::Auxdata::from_cbor(encoded)
            .map(|(_, size)| size)
            .map_err(|_| ())
    }

    fn validate_encoded_length(encoded_length: usize, actual_size: usize) -> bool {
        // Vyper counts 2 length related bytes as part of the auxdata
        encoded_length == actual_size + 2
    }
}

fn add_cbor_auxadata_recursive<T: CborAuxdataType>(
    processed_cbor_auxdata: &mut CborAuxdata,
    code: &[u8],
    modified_code: &[u8],
    mut already_processed: usize,
) -> Result<(), Error> {
    let mismatch_index = code
        .iter()
        .zip(modified_code.iter())
        .skip(already_processed)
        .position(|(original, modified)| original != modified);

    if mismatch_index.is_none() {
        return Ok(());
    }
    let mismatch_index = mismatch_index.unwrap() + already_processed;

    let next_cbor_auxdata_value =
        next_cbor_auxdata_value::<T>(code, already_processed, mismatch_index)?;

    already_processed =
        next_cbor_auxdata_value.offset as usize + next_cbor_auxdata_value.value.len();

    let key = processed_cbor_auxdata.len() + 1;
    processed_cbor_auxdata.insert(key.to_string(), next_cbor_auxdata_value);

    add_cbor_auxadata_recursive::<T>(
        processed_cbor_auxdata,
        code,
        modified_code,
        already_processed,
    )
}

fn next_cbor_auxdata_value<T: CborAuxdataType>(
    code: &[u8],
    already_processed: usize,
    mut start: usize,
) -> Result<CborAuxdataValue, Error> {
    let length = loop {
        let mut result = T::from_cbor(&code[start..]);
        while result.is_err() {
            if start == already_processed {
                Err(anyhow::anyhow!("failed to parse next cbor auxdata value"))?
            }
            start -= 1;
            result = T::from_cbor(&code[start..]);
        }
        let length = result.unwrap();

        if is_cbor_auxdata_length_valid::<T>(code, start, length) {
            break length;
        }

        start -= 1;
    };

    let value = CborAuxdataValue {
        value: code[start..start + length + 2].to_vec(),
        offset: start as u32,
    };
    Ok(value)
}

fn is_cbor_auxdata_length_valid<T: CborAuxdataType>(
    code: &[u8],
    start: usize,
    length: usize,
) -> bool {
    if code.len() < start + length + 2
    /* 2 length bytes are not parsed and included in length */
    {
        return false;
    }

    let mut encoded_length_bytes = [0u8; 2];
    encoded_length_bytes.copy_from_slice(&code[start + length..start + length + 2]);
    let encoded_length = u16::from_be_bytes(encoded_length_bytes) as usize;

    T::validate_encoded_length(encoded_length, length)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use verification_common::verifier_alliance::CborAuxdataValue;

    const SOLC_MAIN_PART_ONE: &str = "608060405234801561001057600080fd5b506040518060200161002190610050565b6020820181038252601f19601f820116604052506000908051906020019061004a92919061005c565b5061015f565b605c806101ac83390190565b8280546100689061012e565b90600052602060002090601f01602090048101928261008a57600085556100d1565b82601f106100a357805160ff19168380011785556100d1565b828001600101855582156100d1579182015b828111156100d05782518255916020019190600101906100b5565b5b5090506100de91906100e2565b5090565b5b808211156100fb5760008160009055506001016100e3565b5090565b7f4e487b7100000000000000000000000000000000000000000000000000000000600052602260045260246000fd5b6000600282049050600182168061014657607f821691505b602082108103610159576101586100ff565b5b50919050565b603f8061016d6000396000f3fe6080604052600080fdfe";
    const SOLC_MAIN_PART_TWO: &str =
        "6080604052348015600f57600080fd5b50603f80601d6000396000f3fe6080604052600080fdfe";
    const SOLC_CBOR_AUXDATA_PART_ONE: &str = "a26469706673582212202e82fb6222f966f0e56dc49cd1fb8a6b5eac9bdf74f62b8a5e9d8812901095d664736f6c634300080e0033";
    const SOLC_CBOR_AUXDATA_PART_TWO: &str = "a2646970667358221220bd9f7fd5fb164e10dd86ccc9880d27a177e74ba873e6a9b97b6c4d7062b26ff064736f6c634300080e0033";
    const SOLC_CBOR_AUXDATA_PART_THREE: &str = "a264697066735822122028c67e368422bc9c0b12226a099aa62a1facd39b08a84427d7f3efe1e37029b864736f6c634300080e0033";
    const SOLC_CBOR_AUXADAT_PART_FOUR: &str = "a26469706673582212206b331720b143820ca2e65d7db53a1b005672433fcb7f2da3ab539851bddc226a64736f6c634300080e0033";

    fn decode_hex(hex: &str) -> Vec<u8> {
        blockscout_display_bytes::decode_hex(hex).unwrap()
    }

    fn build_cbor_auxdata_from_values(values: &[CborAuxdataValue]) -> CborAuxdata {
        let mut cbor_auxdata = CborAuxdata::new();
        for (index, value) in values.iter().enumerate() {
            cbor_auxdata.insert((index + 1).to_string(), value.clone());
        }
        cbor_auxdata
    }

    #[test]
    fn should_retrieve_solc_cbor_auxdata() {
        let code = decode_hex(&format!("{SOLC_MAIN_PART_ONE}{SOLC_CBOR_AUXDATA_PART_ONE}"));
        let modified_code = decode_hex(&format!(
            "{SOLC_MAIN_PART_ONE}{SOLC_CBOR_AUXDATA_PART_THREE}"
        ));

        let auxdata_values = [CborAuxdataValue {
            offset: SOLC_MAIN_PART_ONE.len() as u32 / 2,
            value: decode_hex(SOLC_CBOR_AUXDATA_PART_ONE),
        }];
        let expected = build_cbor_auxdata_from_values(&auxdata_values);

        let actual = retrieve_cbor_auxdata(Language::Solidity, &code, &modified_code)
            .expect("error retrieving cbor auxdata");
        assert_eq!(Some(expected), actual);
    }

    #[test]
    fn should_retrieve_several_solc_cbor_auxdata() {
        let code = decode_hex(&format!(
            "{SOLC_MAIN_PART_ONE}{SOLC_CBOR_AUXDATA_PART_ONE}{SOLC_MAIN_PART_TWO}{SOLC_CBOR_AUXDATA_PART_TWO}"
        ));
        let modified_code = decode_hex(&format!(
            "{SOLC_MAIN_PART_ONE}{SOLC_CBOR_AUXDATA_PART_THREE}{SOLC_MAIN_PART_TWO}{SOLC_CBOR_AUXADAT_PART_FOUR}"
        ));

        let auxdata_values = [
            CborAuxdataValue {
                offset: SOLC_MAIN_PART_ONE.len() as u32 / 2,
                value: decode_hex(SOLC_CBOR_AUXDATA_PART_ONE),
            },
            CborAuxdataValue {
                offset: (SOLC_MAIN_PART_ONE.len()
                    + SOLC_CBOR_AUXDATA_PART_ONE.len()
                    + SOLC_MAIN_PART_TWO.len()) as u32
                    / 2,
                value: decode_hex(SOLC_CBOR_AUXDATA_PART_TWO),
            },
        ];
        let expected = build_cbor_auxdata_from_values(&auxdata_values);

        let actual = retrieve_cbor_auxdata(Language::Solidity, &code, &modified_code)
            .expect("error retrieving cbor auxdata");

        assert_eq!(Some(expected), actual);
    }

    #[test]
    fn should_return_no_cbor_auxdata_if_codes_are_similar() {
        let code = decode_hex(SOLC_MAIN_PART_ONE);
        let modified_code = decode_hex(SOLC_MAIN_PART_ONE);

        let expected = build_cbor_auxdata_from_values(&[]);

        let actual = retrieve_cbor_auxdata(Language::Solidity, &code, &modified_code)
            .expect("error retrieving cbor auxdata");
        assert_eq!(Some(expected), actual);
    }

    const VYPER_MAIN_PART_ONE: &str = "60405150346100245760206100875f395f5160405261002a61002860163961004a6016f35b5f80fd5f3560e01c633fa4f2458118610022573461002657602061002a60403960206040f35b5f5ffd5b5f80fd";
    const VYPER_CBOR_AUXDATA_PART_ONE: &str = "8558203aae1eb6ffcb8f8c87fd598b19f959d0abcc45d96a586f04c3fd37de50a51448182a801820a1657679706572830004010035";
    const VYPER_CBOR_AUXDATA_PART_TWO: &str = "85582062e442e07645e52a62895c4b40ba3865b89c53b315e2a157ab05171589dcb71d182a801820a1657679706572830004010035";

    #[test]
    fn should_retrieve_vyper_cbor_auxdata() {
        let code = decode_hex(&format!(
            "{VYPER_MAIN_PART_ONE}{VYPER_CBOR_AUXDATA_PART_ONE}"
        ));
        let modified_code = decode_hex(&format!(
            "{VYPER_MAIN_PART_ONE}{VYPER_CBOR_AUXDATA_PART_TWO}"
        ));

        let auxdata_values = [CborAuxdataValue {
            offset: VYPER_MAIN_PART_ONE.len() as u32 / 2,
            value: decode_hex(VYPER_CBOR_AUXDATA_PART_ONE),
        }];
        let expected = build_cbor_auxdata_from_values(&auxdata_values);

        let actual = retrieve_cbor_auxdata(Language::Vyper, &code, &modified_code)
            .expect("error retrieving cbor auxdata");
        assert_eq!(Some(expected), actual);
    }

    #[test]
    fn should_return_no_cbor_auxadata_if_codes_are_similar() {
        let code = decode_hex(&format!(
            "{VYPER_MAIN_PART_ONE}{VYPER_CBOR_AUXDATA_PART_ONE}"
        ));
        let modified_code = decode_hex(&format!(
            "{VYPER_MAIN_PART_ONE}{VYPER_CBOR_AUXDATA_PART_ONE}"
        ));

        let expected = build_cbor_auxdata_from_values(&[]);

        let actual = retrieve_cbor_auxdata(Language::Solidity, &code, &modified_code)
            .expect("error retrieving cbor auxdata");
        assert_eq!(Some(expected), actual);
    }
}
