use crate::verifier_alliance::Match;
use anyhow::Context;
use blockscout_display_bytes::ToHex;
use std::collections::BTreeMap;

pub fn parse_manually_linked_libraries(match_: &Match) -> BTreeMap<String, String> {
    let mut libraries = BTreeMap::new();
    match_
        .values
        .libraries
        .iter()
        .for_each(|(fully_qualified_name, address)| {
            libraries.insert(fully_qualified_name.clone(), address.to_hex());
        });
    libraries
}

pub fn try_parse_compiler_linked_libraries(
    compiler_settings: &serde_json::Value,
) -> Result<BTreeMap<String, String>, anyhow::Error> {
    let libraries = compiler_settings
        .pointer("/libraries")
        .map(|value| {
            let libraries: BTreeMap<String, BTreeMap<String, String>> =
                serde::Deserialize::deserialize(value)
                    .context("cannot parse linked libraries")
                    .inspect_err(|err| tracing::error!("{err:#?}"))?;
            let compressed_libraries = libraries.into_iter().fold(
                BTreeMap::new(),
                |mut compressed_libraries, (file_name, file_libraries)| {
                    file_libraries
                        .into_iter()
                        .for_each(|(contract_name, address)| {
                            let fully_qualified_name = format!("{file_name}:{contract_name}");
                            compressed_libraries.insert(fully_qualified_name, address);
                        });
                    compressed_libraries
                },
            );
            Ok::<_, anyhow::Error>(compressed_libraries)
        })
        .transpose()?;

    Ok(libraries.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn can_parse_manually_linked_libraries() {
        let libraries: BTreeMap<String, String> = BTreeMap::from([
            (
                "Swaps.sol:Swaps".into(),
                "0x2b2bfe80547f50e1a67bbf0d52c24e0683f67b6d".into(),
            ),
            (
                "Curve.sol:Curves".into(),
                "0x017deac7bceec08aca1e71acb639065a3492e830".into(),
            ),
            (
                "Orchestrator.sol:Orchestrator".into(),
                "0xa0f599414c0f66e372200b16e9533c9c9e777fdd".into(),
            ),
        ]);

        let match_values = json!({
            "libraries": libraries
        });
        let match_ = Match {
            metadata_match: false,
            transformations: vec![],
            values: serde_json::from_value(match_values).unwrap(),
        };

        let parsed = parse_manually_linked_libraries(&match_);
        assert_eq!(libraries, parsed);
    }

    #[test]
    fn can_parse_no_manually_linked_libraries() {
        let expected = BTreeMap::new();
        let match_values = json!({
            "libraries": expected
        });
        let match_ = Match {
            metadata_match: false,
            transformations: vec![],
            values: serde_json::from_value(match_values).unwrap(),
        };

        let parsed = parse_manually_linked_libraries(&match_);
        assert_eq!(expected, parsed);
    }

    #[test]
    fn can_parse_compiler_linked_libraries() {
        let settings = json!({
            "viaIR": true,
            "metadata": {"bytecodeHash": "ipfs"},
            "libraries": {
                "src/lib/BunniSwapMath.sol": {"BunniSwapMath": "0x00000000af7929ae27a7aa6e9eb68fe0f10bba88"},
                "src/lib/BunniHookLogic.sol": {"BunniHookLogic": "0x11f2ee6b0fc6367efae53e0a33ee6216974a0f81"},
                "src/lib/RebalanceLogic.sol": {"RebalanceLogic": "0x000000005bea54b06ba1376474cf0df55f608200"}
            },
            "optimizer": {"runs": 7500, "enabled": true},
            "evmVersion": "cancun",
            "remappings": []
        });
        let parsed = try_parse_compiler_linked_libraries(&settings)
            .expect("cannot parse compiler linked libraries");

        let expected: BTreeMap<String, String> = BTreeMap::from([
            (
                "src/lib/BunniSwapMath.sol:BunniSwapMath".into(),
                "0x00000000af7929ae27a7aa6e9eb68fe0f10bba88".into(),
            ),
            (
                "src/lib/BunniHookLogic.sol:BunniHookLogic".into(),
                "0x11f2ee6b0fc6367efae53e0a33ee6216974a0f81".into(),
            ),
            (
                "src/lib/RebalanceLogic.sol:RebalanceLogic".into(),
                "0x000000005bea54b06ba1376474cf0df55f608200".into(),
            ),
        ]);
        assert_eq!(expected, parsed);
    }

    #[test]
    fn can_parse_no_compiler_linked_libraries() {
        let settings = json!({
            "metadata": {"bytecodeHash": "ipfs"},
            "libraries": {},
            "optimizer": {"runs": 9999, "enabled": true},
            "evmVersion": "istanbul",
            "remappings": []
        });
        let parsed = try_parse_compiler_linked_libraries(&settings)
            .expect("cannot parse compiler linked libraries");

        let expected: BTreeMap<String, String> = BTreeMap::new();
        assert_eq!(expected, parsed);
    }
}
