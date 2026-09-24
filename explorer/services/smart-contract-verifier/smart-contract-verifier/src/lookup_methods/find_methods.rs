use super::{
    disassemble::{disassemble_bytecode, DisassembledOpcode},
    method::Method,
};
use alloy_json_abi::JsonAbi;
use bytes::Bytes;
use foundry_compilers::artifacts::sourcemap::SourceMap;
use std::{collections::BTreeMap, iter::repeat_n};

pub struct LookupMethodsRequest {
    pub bytecode: Bytes,
    pub abi: JsonAbi,
    pub source_map: SourceMap,
    pub file_ids: BTreeMap<u32, String>,
}

pub struct LookupMethodsResponse {
    pub methods: BTreeMap<String, Method>,
}

pub fn find_methods(request: LookupMethodsRequest) -> LookupMethodsResponse {
    let methods = parse_selectors(request.abi);
    let opcodes = disassemble_bytecode(&request.bytecode);

    let methods = methods
        .into_iter()
        .filter_map(|(func_sig, selector)| {
            let func_index = match find_src_map_index(&selector, &opcodes) {
                Some(i) => i,
                None => {
                    tracing::warn!(func_sig, "function not found");
                    return None;
                }
            };

            tracing::debug!(func_sig, func_index, "found function");
            let method = match Method::from_source_map(
                selector,
                &request.source_map,
                func_index,
                &request.file_ids,
            ) {
                Ok(m) => m,
                Err(err) => {
                    tracing::warn!(func_sig, err = err.to_string(), "failed to parse method");
                    return None;
                }
            };
            Some((hex::encode(selector), method))
        })
        .collect();
    LookupMethodsResponse { methods }
}

fn find_src_map_index(selector: &[u8; 4], opcodes: &[DisassembledOpcode]) -> Option<usize> {
    for window in opcodes.windows(5) {
        if window[0].operation.name.starts_with("PUSH")
            && window[1].operation.name == "EQ"
            && window[2].operation.name.starts_with("PUSH")
            && window[3].operation.name == "JUMPI"
        {
            // If found selector doesn't match, continue
            if !prepend_selector(&window[0].args).is_ok_and(|s| s == selector) {
                continue;
            }

            let jump_to = usize::from_str_radix(&hex::encode(&window[2].args), 16).ok()?;

            let maybe_target_opcode_index = opcodes
                .iter()
                .enumerate()
                .find_map(|(index, opcode)| (opcode.program_counter == jump_to).then_some(index));

            match maybe_target_opcode_index {
                Some(index) => return Some(index),
                None => tracing::warn!(selector =? selector, "target opcode not found"),
            }
        }

        if window[0].operation.name.starts_with("PUSH")
            && window[1].operation.name == "DUP2"
            && window[2].operation.name == "EQ"
            && window[3].operation.name.starts_with("PUSH")
            && window[4].operation.name == "JUMPI"
        {
            // If found selector doesn't match, continue
            if !prepend_selector(&window[0].args).is_ok_and(|s| s == selector) {
                continue;
            }

            let jump_to = usize::from_str_radix(&hex::encode(&window[3].args), 16).ok()?;

            let maybe_target_opcode_index = opcodes
                .iter()
                .enumerate()
                .find_map(|(index, opcode)| (opcode.program_counter == jump_to).then_some(index));

            match maybe_target_opcode_index {
                Some(index) => return Some(index),
                None => tracing::warn!(selector =? selector, "target opcode not found"),
            }
        }
    }

    None
}

fn parse_selectors(abi: JsonAbi) -> BTreeMap<String, [u8; 4]> {
    abi.functions()
        .map(|f| (f.signature(), f.selector().0))
        .collect()
}

fn prepend_selector(partial_selector: &[u8]) -> anyhow::Result<Vec<u8>> {
    if partial_selector.len() > 4 {
        return Err(anyhow::anyhow!("selector is too long"));
    };

    // prepend selector with 0s if it's shorter than 4 bytes
    let mut selector = partial_selector.to_owned();
    selector.splice(..0, repeat_n(0, 4 - partial_selector.len()));
    Ok(selector)
}

#[cfg(test)]
mod tests {
    use super::prepend_selector;

    #[test]
    fn test_prepend_selector() {
        assert_eq!(prepend_selector(&[1, 2, 3, 4]).unwrap(), vec![1, 2, 3, 4]);
        assert_eq!(prepend_selector(&[1, 2]).unwrap(), vec![0, 0, 1, 2]);
        assert!(prepend_selector(&[1, 2, 3, 4, 5]).is_err());
    }
}
