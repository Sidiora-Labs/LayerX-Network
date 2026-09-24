use foundry_compilers::artifacts::{
    output_selection::{FileOutputSelection, OutputSelection},
    serde_helpers, EvmVersion, Source, Sources,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

pub type Interfaces = BTreeMap<PathBuf, Interface>;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VyperInput {
    pub language: String,
    pub sources: Sources,
    #[serde(default)]
    pub interfaces: Interfaces,
    #[serde(default)]
    pub settings: Settings,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(
        default,
        with = "serde_helpers::display_from_str_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub evm_version: Option<EvmVersion>,
    /// Indicates whether or not optimizations are turned on.
    /// This is true by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub optimize: Option<bool>,
    /// Indicates whether or not the bytecode should include Vyper's signature.
    /// This is true by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytecode_metadata: Option<bool>,
    /// This field can be used to select desired outputs based
    /// on file and contract names.
    /// If this field is omitted, then the compiler loads and does type
    /// checking, but will not generate any outputs apart from errors.
    #[serde(default)]
    pub output_selection: FileOutputSelection,
    #[serde(
        rename = "search_paths",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub search_paths: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            output_selection: OutputSelection::default_file_output_selection(),
            evm_version: None,
            optimize: None,
            bytecode_metadata: None,
            search_paths: Vec::new(),
        }
    }
}

mod interfaces {
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
    #[serde(rename_all = "camelCase")]
    pub struct AbiSource {
        pub abi: serde_json::Value,
    }

    #[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
    #[serde(rename_all = "camelCase")]
    pub struct ContractTypesSource {
        pub contract_types: serde_json::Value,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(untagged)]
pub enum Interface {
    Vyper(Source),
    Abi(interfaces::AbiSource),
    ContractTypes(interfaces::ContractTypesSource),
}

impl Interface {
    pub fn try_new(path: &Path, content: String) -> Result<Self, anyhow::Error> {
        // Adapted from: https://github.com/vyperlang/vyper/blob/v0.3.9/vyper/cli/vyper_compile.py#L217-L246
        if path.extension() == Some(std::ffi::OsStr::new("json")) {
            let content: serde_json::Value = serde_json::from_str(&content).map_err(|err| {
                anyhow::anyhow!(
                    "couldn't parse the content of an interface as a json value: {path:?} - {err}"
                )
            })?;
            match content {
                serde_json::Value::Object(map) if map.contains_key("contractTypes") => {
                    let contract_types = map.get("contractTypes").unwrap().clone();
                    Ok(Interface::ContractTypes(interfaces::ContractTypesSource {
                        contract_types,
                    }))
                }
                serde_json::Value::Object(map) if map.contains_key("abi") => {
                    let abi = map.get("abi").unwrap().clone();
                    Ok(Interface::Abi(interfaces::AbiSource { abi }))
                }
                serde_json::Value::Array(_) => {
                    Ok(Interface::Abi(interfaces::AbiSource { abi: content }))
                }
                _ => Err(anyhow::anyhow!("\"{path:?}\" is an invalid interface")),
            }
        } else {
            Ok(Interface::Vyper(Source::new(content)))
        }
    }

    pub fn content(self) -> String {
        match self {
            Interface::Vyper(source) => Arc::unwrap_or_clone(source.content),
            Interface::Abi(source) => serde_json::to_string(&source.abi).unwrap(),
            Interface::ContractTypes(source) => {
                format!(
                    r#"{{"contractTypes":{}}}"#,
                    serde_json::to_string(&source.contract_types).unwrap()
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const COMPILER_INPUT_TEMPLATE: &str = r#"
        {
            "language": "Vyper",
            "sources": {
                "contracts/Arbitrage.vy": {
                    "content": " # @version 0.3.7\r\n\r\nfrom ..interfaces import ERC20\r\nfrom ..interfaces.ContractTypes import SFRXETH\r\nfrom ..interfaces import Router as ROUTER\r\n\r\ninterface LLAMMA:\r\n    def exchange(i: uint256, j: uint256, in_amount: uint256, min_amount: uint256, _for: address = msg.sender) -> uint256[2]: nonpayable\r\n    def get_dy(i: uint256, j: uint256, in_amount: uint256) -> uint256: view\r\n    def get_dxdy(i: uint256, j: uint256, in_amount: uint256) -> uint256[2]: view\r\n\r\nfrxeth: constant(address) = 0x5E8422345238F34275888049021821E8E08CAa1f\r\nsfrxeth: constant(address) = 0xac3E018457B222d93114458476f3E3416Abbe38F\r\n\r\nllamma: public(address)\r\nrouter: public(address)\r\ncrvusd: public(address)\r\n\r\nadmin: public(address)\r\n\r\n\r\n@external\r\ndef __init__(_llamma: address, _router: address, _crvusd: address):\r\n    self.llamma = _llamma\r\n    self.router = _router\r\n    self.crvusd = _crvusd\r\n    self.admin = msg.sender\r\n\r\n    ERC20(_crvusd).approve(_llamma, max_value(uint256), default_return_value=True)\r\n    ERC20(_crvusd).approve(_router, max_value(uint256), default_return_value=True)\r\n    SFRXETH(sfrxeth).approve(_llamma, max_value(uint256), default_return_value=True)\r\n    ERC20(frxeth).approve(_router, max_value(uint256), default_return_value=True)\r\n    ERC20(frxeth).approve(sfrxeth, max_value(uint256), default_return_value=True)\r\n\r\n\r\n@view\r\n@external\r\ndef convert_to_assets(shares: uint256) -> uint256:\r\n    return SFRXETH(sfrxeth).convertToAssets(shares)\r\n\r\n\r\n@view\r\n@external\r\n@nonreentrant('lock')\r\ndef calc_output(in_amount: uint256, liquidation: bool, _route: address[9], _swap_params: uint256[3][4], _pools: address[4]) -> uint256[3]:\r\n    \"\"\"\r\n    @notice Calculate liquidator profit\r\n    @param in_amount Amount of collateral going in\r\n    @param liquidation Liquidation or de-liquidation\r\n    @param _route Arg for router\r\n    @param _swap_params Arg for router\r\n    @param _pools Arg for router\r\n    @return (amount of collateral going out, amount of crvUSD in the middle, amount of crvUSD\/collateral DONE)\r\n    \"\"\"\r\n    output: uint256 = 0\r\n    crv_usd: uint256 = 0\r\n    done: uint256 = 0\r\n    if liquidation:\r\n        # collateral --> ROUTER --> crvUSD --> LLAMMA --> collateral\r\n        frxeth_amount: uint256 = SFRXETH(sfrxeth).convertToAssets(in_amount)\r\n        crv_usd = ROUTER(self.router).get_exchange_multiple_amount(_route, _swap_params, frxeth_amount, _pools)\r\n        dxdy: uint256[2] = LLAMMA(self.llamma).get_dxdy(0, 1, crv_usd)\r\n        done = dxdy[0]  # crvUSD\r\n        output = dxdy[1]\r\n    else:\r\n        # de-liquidation\r\n        # collateral --> LLAMMA --> crvUSD --> ROUTER --> collateral\r\n        dxdy: uint256[2] = LLAMMA(self.llamma).get_dxdy(1, 0, in_amount)\r\n        done = dxdy[0]  # collateral\r\n        crv_usd = dxdy[1]\r\n        output = ROUTER(self.router).get_exchange_multiple_amount(_route, _swap_params, crv_usd, _pools)\r\n        output = SFRXETH(sfrxeth).convertToShares(output)\r\n\r\n    return [output, crv_usd, done]\r\n\r\n\r\n@external\r\n@nonreentrant('lock')\r\ndef exchange(\r\n        in_amount: uint256,\r\n        min_crv_usd: uint256,\r\n        min_output: uint256,\r\n        liquidation: bool,\r\n        _route: address[9],\r\n        _swap_params: uint256[3][4],\r\n        _pools: address[4],\r\n        _for: address = msg.sender,\r\n) -> uint256[2]:\r\n    assert SFRXETH(sfrxeth).transferFrom(msg.sender, self, in_amount, default_return_value=True)\r\n\r\n    if liquidation:\r\n        # collateral --> ROUTER --> crvUSD --> LLAMMA --> collateral\r\n        frxeth_amount: uint256 = SFRXETH(sfrxeth).redeem(in_amount, self, self)\r\n        crv_usd: uint256 = ROUTER(self.router).exchange_multiple(_route, _swap_params, frxeth_amount, min_crv_usd, _pools)\r\n        LLAMMA(self.llamma).exchange(0, 1, crv_usd, min_output)\r\n    else:\r\n        # de-liquidation\r\n        # collateral --> LLAMMA --> crvUSD --> ROUTER --> collateral\r\n        out_in: uint256[2] = LLAMMA(self.llamma).exchange(1, 0, in_amount, min_crv_usd)\r\n        crv_usd: uint256 = out_in[1]\r\n        output: uint256 = ROUTER(self.router).exchange_multiple(_route, _swap_params, crv_usd, min_output, _pools)\r\n        SFRXETH(sfrxeth).deposit(output, self)\r\n\r\n    collateral_balance: uint256 = SFRXETH(sfrxeth).balanceOf(self)\r\n    SFRXETH(sfrxeth).transfer(_for, collateral_balance)\r\n    crv_usd_balance: uint256 = ERC20(self.crvusd).balanceOf(self)\r\n    ERC20(self.crvusd).transfer(_for, crv_usd_balance)\r\n\r\n    return [collateral_balance, crv_usd_balance]\r\n\r\n\r\n@external\r\n@nonreentrant('lock')\r\ndef set_llamma(_llamma: address):\r\n    assert msg.sender == self.admin, \"admin only\"\r\n    self.llamma = _llamma\r\n\r\n    ERC20(self.crvusd).approve(_llamma, max_value(uint256), default_return_value=True)\r\n    SFRXETH(sfrxeth).approve(_llamma, max_value(uint256), default_return_value=True)\r\n\r\n\r\n@external\r\n@nonreentrant('lock')\r\ndef set_crvusd(_crvusd: address):\r\n    assert msg.sender == self.admin, \"admin only\"\r\n    self.crvusd = _crvusd\r\n\r\n    ERC20(_crvusd).approve(self.llamma, max_value(uint256), default_return_value=True)\r\n    ERC20(_crvusd).approve(self.router, max_value(uint256), default_return_value=True)\r\n\r\n\r\n@external\r\n@nonreentrant('lock')\r\ndef set_llamma_and_crvusd(_llamma: address, _crvusd: address):\r\n    assert msg.sender == self.admin, \"admin only\"\r\n    self.llamma = _llamma\r\n    self.crvusd = _crvusd\r\n\r\n    ERC20(_crvusd).approve(_llamma, max_value(uint256), default_return_value=True)\r\n    ERC20(_crvusd).approve(self.router, max_value(uint256), default_return_value=True)\r\n    SFRXETH(sfrxeth).approve(_llamma, max_value(uint256), default_return_value=True)\r\n\r\n\r\n@external\r\n@nonreentrant('lock')\r\ndef set_router(_router: address):\r\n    assert msg.sender == self.admin, \"admin only\"\r\n    self.router = _router\r\n\r\n    ERC20(self.crvusd).approve(_router, max_value(uint256), default_return_value=True)\r\n    ERC20(frxeth).approve(_router, max_value(uint256), default_return_value=True)\r\n"
                }
            },
            "interfaces": {
                "interfaces/Router.vy": {
                    "content": "@external\r\n@payable\r\ndef exchange_multiple(_route: address[9], _swap_params: uint256[3][4], _amount: uint256, _expected: uint256, _pools: address[4]) -> uint256: \r\n    pass\r\n\r\n@external\r\n@view\r\ndef get_exchange_multiple_amount(_route: address[9], _swap_params: uint256[3][4], _amount: uint256, _pools: address[4]) -> uint256: \r\n    pass"
                },
                "interfaces/ERC20.json": {
                    "abi": [{"inputs":[{"name":"_to","type":"address"},{"name":"_value","type":"uint256"}],"name":"transfer","outputs":[{"name":"","type":"bool"}],"stateMutability":"nonpayable","type":"function"},{"inputs":[{"name":"_from","type":"address"},{"name":"_to","type":"address"},{"name":"_value","type":"uint256"}],"name":"transferFrom","outputs":[{"name":"","type":"bool"}],"stateMutability":"nonpayable","type":"function"},{"inputs":[{"name":"_spender","type":"address"},{"name":"_value","type":"uint256"}],"name":"approve","outputs":[{"name":"","type":"bool"}],"stateMutability":"nonpayable","type":"function"},{"inputs":[{"name":"_for","type":"address"}],"name":"balanceOf","outputs":[{"name":"","type":"uint256"}],"stateMutability":"view","type":"function"}]
                },
                "interfaces/ContractTypes.json": {
                    "contractTypes": {
                        "SFRXETH": {
                            "abi": [{"stateMutability": "nonpayable", "type": "function", "name": "transfer", "inputs": [{"name": "_to", "type": "address"}, {"name": "_value", "type": "uint256"}], "outputs": [{"name": "", "type": "bool"}]}, {"stateMutability": "nonpayable", "type": "function", "name": "transferFrom", "inputs": [{"name": "_from", "type": "address"}, {"name": "_to", "type": "address"}, {"name": "_value", "type": "uint256"}], "outputs": [{"name": "", "type": "bool"}]}, {"stateMutability": "nonpayable", "type": "function", "name": "approve", "inputs": [{"name": "_spender", "type": "address"}, {"name": "_value", "type": "uint256"}], "outputs": [{"name": "", "type": "bool"}]}, {"stateMutability": "view", "type": "function", "name": "balanceOf", "inputs": [{"name": "_for", "type": "address"}], "outputs": [{"name": "", "type": "uint256"}]}, {"stateMutability": "view", "type": "function", "name": "convertToShares", "inputs": [{"name": "assets", "type": "uint256"}], "outputs": [{"name": "", "type": "uint256"}]}, {"stateMutability": "view", "type": "function", "name": "convertToAssets", "inputs": [{"name": "shares", "type": "uint256"}], "outputs": [{"name": "", "type": "uint256"}]}, {"stateMutability": "nonpayable", "type": "function", "name": "deposit", "inputs": [{"name": "assets", "type": "uint256"}, {"name": "receiver", "type": "address"}], "outputs": [{"name": "", "type": "uint256"}]}, {"stateMutability": "nonpayable", "type": "function", "name": "redeem", "inputs": [{"name": "shares", "type": "uint256"}, {"name": "receiver", "type": "address"}, {"name": "owner", "type": "address"}], "outputs": [{"name": "", "type": "uint256"}]}]
                        }
                    }
                }
            },
            "settings": {SETTINGS}
        }
    "#;

    const SETTINGS: [&str; 3] = [
        "{}",
        r#"{
            "evmVersion": "istanbul",
            "optimize": false,
            "bytecodeMetadata": false,
            "outputSelection": {
                "*": ["evm.bytecode"]
            }
        }"#,
        r#"{
            "evmVersion": "istanbul",
            "outputSelection": {
                "*": ["evm.bytecode"]
            }
        }"#,
    ];

    #[test]
    fn can_parse_standard_json_compiler_input() {
        for settings in SETTINGS {
            let compiler_input = COMPILER_INPUT_TEMPLATE.replace("{SETTINGS}", settings);
            let val = serde_json::from_str::<VyperInput>(&compiler_input).unwrap_or_else(|err| {
                panic!("Failed to read compiler input: {compiler_input} - {err}")
            });

            let pretty = serde_json::to_string_pretty(&val).unwrap();
            serde_json::from_str::<VyperInput>(&pretty).unwrap_or_else(|err| {
                panic!("Failed to read converted compiler input: {pretty} - {err}")
            });
        }
    }

    #[test]
    fn can_create_interface() {
        let vyper_data = ("interfaces/Router.vy", "@external\r\n@payable\r\ndef exchange_multiple(_route: address[9], _swap_params: uint256[3][4], _amount: uint256, _expected: uint256, _pools: address[4]) -> uint256: \r\n    pass\r\n\r\n@external\r\n@view\r\ndef get_exchange_multiple_amount(_route: address[9], _swap_params: uint256[3][4], _amount: uint256, _pools: address[4]) -> uint256: \r\n    pass");
        let contract_types_data = (
            "interfaces/ContractTypes.json",
            r#"{
                "SFRXETH": {
                    "abi": [{"stateMutability": "nonpayable", "type": "function", "name": "transfer", "inputs": [{"name": "_to", "type": "address"}, {"name": "_value", "type": "uint256"}], "outputs": [{"name": "", "type": "bool"}]}, {"stateMutability": "nonpayable", "type": "function", "name": "transferFrom", "inputs": [{"name": "_from", "type": "address"}, {"name": "_to", "type": "address"}, {"name": "_value", "type": "uint256"}], "outputs": [{"name": "", "type": "bool"}]}, {"stateMutability": "nonpayable", "type": "function", "name": "approve", "inputs": [{"name": "_spender", "type": "address"}, {"name": "_value", "type": "uint256"}], "outputs": [{"name": "", "type": "bool"}]}, {"stateMutability": "view", "type": "function", "name": "balanceOf", "inputs": [{"name": "_for", "type": "address"}], "outputs": [{"name": "", "type": "uint256"}]}, {"stateMutability": "view", "type": "function", "name": "convertToShares", "inputs": [{"name": "assets", "type": "uint256"}], "outputs": [{"name": "", "type": "uint256"}]}, {"stateMutability": "view", "type": "function", "name": "convertToAssets", "inputs": [{"name": "shares", "type": "uint256"}], "outputs": [{"name": "", "type": "uint256"}]}, {"stateMutability": "nonpayable", "type": "function", "name": "deposit", "inputs": [{"name": "assets", "type": "uint256"}, {"name": "receiver", "type": "address"}], "outputs": [{"name": "", "type": "uint256"}]}, {"stateMutability": "nonpayable", "type": "function", "name": "redeem", "inputs": [{"name": "shares", "type": "uint256"}, {"name": "receiver", "type": "address"}, {"name": "owner", "type": "address"}], "outputs": [{"name": "", "type": "uint256"}]}]
                }
            }"#,
        );
        let abi_data = (
            "interfaces/ERC20.json",
            r#"[{"inputs":[{"name":"_to","type":"address"},{"name":"_value","type":"uint256"}],"name":"transfer","outputs":[{"name":"","type":"bool"}],"stateMutability":"nonpayable","type":"function"},{"inputs":[{"name":"_from","type":"address"},{"name":"_to","type":"address"},{"name":"_value","type":"uint256"}],"name":"transferFrom","outputs":[{"name":"","type":"bool"}],"stateMutability":"nonpayable","type":"function"},{"inputs":[{"name":"_spender","type":"address"},{"name":"_value","type":"uint256"}],"name":"approve","outputs":[{"name":"","type":"bool"}],"stateMutability":"nonpayable","type":"function"},{"inputs":[{"name":"_for","type":"address"}],"name":"balanceOf","outputs":[{"name":"","type":"uint256"}],"stateMutability":"view","type":"function"}]"#,
        );

        let test_data = [
            (
                vyper_data.0,
                vyper_data.1.to_string(),
                Interface::Vyper(Source::new(vyper_data.1)),
            ),
            (
                contract_types_data.0,
                format!(r#"{{ "contractTypes": {} }}"#, contract_types_data.1),
                Interface::ContractTypes(interfaces::ContractTypesSource {
                    contract_types: serde_json::from_str(contract_types_data.1).unwrap(),
                }),
            ),
            (
                abi_data.0,
                format!(r#"{{ "abi": {} }}"#, abi_data.1),
                Interface::Abi(interfaces::AbiSource {
                    abi: serde_json::from_str(abi_data.1).unwrap(),
                }),
            ),
            (
                abi_data.0,
                abi_data.1.to_string(),
                Interface::Abi(interfaces::AbiSource {
                    abi: serde_json::from_str(abi_data.1).unwrap(),
                }),
            ),
        ];
        for (path, content, expected) in test_data {
            let path = PathBuf::from(path);
            let actual = Interface::try_new(&path, content).unwrap_or_else(|err| {
                panic!("Failed to create interface for path: {path:?} - {err}")
            });
            assert_eq!(expected, actual);
        }
    }

    #[test]
    fn can_return_interface_content() {
        let vyper_content = "@external\r\n@payable\r\ndef exchange_multiple(_route: address[9], _swap_params: uint256[3][4], _amount: uint256, _expected: uint256, _pools: address[4]) -> uint256: \r\n    pass\r\n\r\n@external\r\n@view\r\ndef get_exchange_multiple_amount(_route: address[9], _swap_params: uint256[3][4], _amount: uint256, _pools: address[4]) -> uint256: \r\n    pass";
        let contract_types_content = r#"{
                "SFRXETH": {
                    "abi": [{"stateMutability": "nonpayable", "type": "function", "name": "transfer", "inputs": [{"name": "_to", "type": "address"}, {"name": "_value", "type": "uint256"}], "outputs": [{"name": "", "type": "bool"}]}, {"stateMutability": "nonpayable", "type": "function", "name": "transferFrom", "inputs": [{"name": "_from", "type": "address"}, {"name": "_to", "type": "address"}, {"name": "_value", "type": "uint256"}], "outputs": [{"name": "", "type": "bool"}]}, {"stateMutability": "nonpayable", "type": "function", "name": "approve", "inputs": [{"name": "_spender", "type": "address"}, {"name": "_value", "type": "uint256"}], "outputs": [{"name": "", "type": "bool"}]}, {"stateMutability": "view", "type": "function", "name": "balanceOf", "inputs": [{"name": "_for", "type": "address"}], "outputs": [{"name": "", "type": "uint256"}]}, {"stateMutability": "view", "type": "function", "name": "convertToShares", "inputs": [{"name": "assets", "type": "uint256"}], "outputs": [{"name": "", "type": "uint256"}]}, {"stateMutability": "view", "type": "function", "name": "convertToAssets", "inputs": [{"name": "shares", "type": "uint256"}], "outputs": [{"name": "", "type": "uint256"}]}, {"stateMutability": "nonpayable", "type": "function", "name": "deposit", "inputs": [{"name": "assets", "type": "uint256"}, {"name": "receiver", "type": "address"}], "outputs": [{"name": "", "type": "uint256"}]}, {"stateMutability": "nonpayable", "type": "function", "name": "redeem", "inputs": [{"name": "shares", "type": "uint256"}, {"name": "receiver", "type": "address"}, {"name": "owner", "type": "address"}], "outputs": [{"name": "", "type": "uint256"}]}]
                }
            }"#;
        let abi_content = r#"[{"inputs":[{"name":"_to","type":"address"},{"name":"_value","type":"uint256"}],"name":"transfer","outputs":[{"name":"","type":"bool"}],"stateMutability":"nonpayable","type":"function"},{"inputs":[{"name":"_from","type":"address"},{"name":"_to","type":"address"},{"name":"_value","type":"uint256"}],"name":"transferFrom","outputs":[{"name":"","type":"bool"}],"stateMutability":"nonpayable","type":"function"},{"inputs":[{"name":"_spender","type":"address"},{"name":"_value","type":"uint256"}],"name":"approve","outputs":[{"name":"","type":"bool"}],"stateMutability":"nonpayable","type":"function"},{"inputs":[{"name":"_for","type":"address"}],"name":"balanceOf","outputs":[{"name":"","type":"uint256"}],"stateMutability":"view","type":"function"}]"#;

        let test_data = [
            (
                Interface::Vyper(Source::new(vyper_content)),
                "@external\r\n@payable\r\ndef exchange_multiple(_route: address[9], _swap_params: uint256[3][4], _amount: uint256, _expected: uint256, _pools: address[4]) -> uint256: \r\n    pass\r\n\r\n@external\r\n@view\r\ndef get_exchange_multiple_amount(_route: address[9], _swap_params: uint256[3][4], _amount: uint256, _pools: address[4]) -> uint256: \r\n    pass"
            ),
            (
                Interface::ContractTypes(interfaces::ContractTypesSource {
                    contract_types: serde_json::from_str(contract_types_content).unwrap(),
                }),
                r#"{"contractTypes":{"SFRXETH":{"abi":[{"inputs":[{"name":"_to","type":"address"},{"name":"_value","type":"uint256"}],"name":"transfer","outputs":[{"name":"","type":"bool"}],"stateMutability":"nonpayable","type":"function"},{"inputs":[{"name":"_from","type":"address"},{"name":"_to","type":"address"},{"name":"_value","type":"uint256"}],"name":"transferFrom","outputs":[{"name":"","type":"bool"}],"stateMutability":"nonpayable","type":"function"},{"inputs":[{"name":"_spender","type":"address"},{"name":"_value","type":"uint256"}],"name":"approve","outputs":[{"name":"","type":"bool"}],"stateMutability":"nonpayable","type":"function"},{"inputs":[{"name":"_for","type":"address"}],"name":"balanceOf","outputs":[{"name":"","type":"uint256"}],"stateMutability":"view","type":"function"},{"inputs":[{"name":"assets","type":"uint256"}],"name":"convertToShares","outputs":[{"name":"","type":"uint256"}],"stateMutability":"view","type":"function"},{"inputs":[{"name":"shares","type":"uint256"}],"name":"convertToAssets","outputs":[{"name":"","type":"uint256"}],"stateMutability":"view","type":"function"},{"inputs":[{"name":"assets","type":"uint256"},{"name":"receiver","type":"address"}],"name":"deposit","outputs":[{"name":"","type":"uint256"}],"stateMutability":"nonpayable","type":"function"},{"inputs":[{"name":"shares","type":"uint256"},{"name":"receiver","type":"address"},{"name":"owner","type":"address"}],"name":"redeem","outputs":[{"name":"","type":"uint256"}],"stateMutability":"nonpayable","type":"function"}]}}}"#
            ),
            (
                Interface::Abi(interfaces::AbiSource {
                    abi: serde_json::from_str(abi_content).unwrap(),
                }),
                r#"[{"inputs":[{"name":"_to","type":"address"},{"name":"_value","type":"uint256"}],"name":"transfer","outputs":[{"name":"","type":"bool"}],"stateMutability":"nonpayable","type":"function"},{"inputs":[{"name":"_from","type":"address"},{"name":"_to","type":"address"},{"name":"_value","type":"uint256"}],"name":"transferFrom","outputs":[{"name":"","type":"bool"}],"stateMutability":"nonpayable","type":"function"},{"inputs":[{"name":"_spender","type":"address"},{"name":"_value","type":"uint256"}],"name":"approve","outputs":[{"name":"","type":"bool"}],"stateMutability":"nonpayable","type":"function"},{"inputs":[{"name":"_for","type":"address"}],"name":"balanceOf","outputs":[{"name":"","type":"uint256"}],"stateMutability":"view","type":"function"}]"#
            )
        ];
        for (interface, expected) in test_data {
            let actual = interface.content();
            assert_eq!(expected, actual);
        }
    }
}
