#![allow(dead_code, unused)]

pub struct TestInput {
    pub contract_name: &'static str,
    pub compiler_version: &'static str,
    pub has_constructor_args: bool,
    pub is_yul: bool,
    pub ignore_creation_tx_input: bool,
    pub abi: Option<serde_json::Value>,

    /// If None, the input would be read from the corresponding file
    pub standard_input: Option<String>,
    /// If None, the input would be read from the corresponding file
    pub creation_tx_input: Option<String>,
    /// If None, the bytecode would be read from the corresponding file
    pub deployed_bytecode: Option<String>,
}

impl TestInput {
    pub fn new(contract_name: &'static str, compiler_version: &'static str) -> Self {
        Self {
            contract_name,
            compiler_version,
            has_constructor_args: false,
            is_yul: false,
            ignore_creation_tx_input: false,
            abi: None,

            standard_input: None,
            creation_tx_input: None,
            deployed_bytecode: None,
        }
    }

    pub fn has_constructor_args(mut self) -> Self {
        self.has_constructor_args = true;
        self
    }

    pub fn set_is_yul(mut self) -> Self {
        self.is_yul = true;
        self
    }

    pub fn ignore_creation_tx_input(mut self) -> Self {
        self.ignore_creation_tx_input = true;
        self
    }

    pub fn with_standard_json_input(mut self, standard_json_input: String) -> Self {
        self.standard_input = Some(standard_json_input);
        self
    }

    pub fn with_deployed_bytecode(mut self, deployed_bytecode: String) -> Self {
        self.deployed_bytecode = Some(deployed_bytecode);
        self
    }

    pub fn with_creation_tx_input(mut self, creation_tx_input: String) -> Self {
        self.creation_tx_input = Some(creation_tx_input);
        self
    }
}
