//!
//! The `solc --standard-json` output error.
//!

pub mod source_location;

use std::str::FromStr;

use serde::{Deserialize, Serialize};

use self::source_location::SourceLocation;

///
/// The `solc --standard-json` output error.
///
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Error {
    /// The component type.
    pub component: String,
    /// The error code.
    pub error_code: Option<String>,
    /// The formatted error message.
    pub formatted_message: String,
    /// The non-formatted error message.
    pub message: String,
    /// The error severity.
    pub severity: String,
    /// The error location data.
    pub source_location: Option<SourceLocation>,
    /// The error type.
    pub r#type: String,
}

impl Error {
    ///
    /// Returns the `ecrecover` function usage warning.
    ///
    pub fn message_ecrecover(src: Option<&str>) -> Self {
        let message = r#"
┌──────────────────────────────────────────────────────────────────────────────────────────────────┐
│ Warning: It looks like you are using 'ecrecover' to validate a signature of a user account.      │
│ zkSync Era comes with native account abstraction support, therefore it is highly recommended NOT │
│ to rely on the fact that the account has an ECDSA private key attached to it since accounts might│
│ implement other signature schemes.                                                               │
│ Read more about Account Abstraction at https://v2-docs.zksync.io/dev/developer-guides/aa.html    │
└──────────────────────────────────────────────────────────────────────────────────────────────────┘"#
            .to_owned();

        Self {
            component: "general".to_owned(),
            error_code: None,
            formatted_message: message.clone(),
            message,
            severity: "warning".to_owned(),
            source_location: src.map(SourceLocation::from_str).and_then(Result::ok),
            r#type: "Warning".to_owned(),
        }
    }

    ///
    /// Returns the `<address payable>`'s `send` and `transfer` methods usage error.
    ///
    pub fn message_send_and_transfer(src: Option<&str>) -> Self {
        let message = r#"
┌──────────────────────────────────────────────────────────────────────────────────────────────────┐
│ Warning: It looks like you are using '<address payable>.send/transfer(<X>)' without providing    │
│ the gas amount. Such calls will fail depending on the pubdata costs.                             │
│ This might be a false positive if you are using an interface (like IERC20) instead of the        │
│ native Solidity `send/transfer`.                                                                 │
│ Please use 'payable(<address>).call{value: <X>}("")' instead, but be careful with the reentrancy │
│ attack. `send` and `transfer` send limited amount of gas that prevents reentrancy, whereas       │
│ `<address>.call{value: <X>}` sends all gas to the callee. Learn more on                          │
│ https://docs.soliditylang.org/en/latest/security-considerations.html#reentrancy                  │
└──────────────────────────────────────────────────────────────────────────────────────────────────┘"#
            .to_owned();

        Self {
            component: "general".to_owned(),
            error_code: None,
            formatted_message: message.clone(),
            message,
            severity: "warning".to_owned(),
            source_location: src.map(SourceLocation::from_str).and_then(Result::ok),
            r#type: "Warning".to_owned(),
        }
    }

    ///
    /// Returns the `extcodesize` instruction usage warning.
    ///
    pub fn message_extcodesize(src: Option<&str>) -> Self {
        let message = r#"
┌──────────────────────────────────────────────────────────────────────────────────────────────────┐
│ Warning: Your code or one of its dependencies uses the 'extcodesize' instruction, which is       │
│ usually needed in the following cases:                                                           │
│   1. To detect whether an address belongs to a smart contract.                                   │
│   2. To detect whether the deploy code execution has finished.                                   │
│ zkSync Era comes with native account abstraction support (so accounts are smart contracts,       │
│ including private-key controlled EOAs), and you should avoid differentiating between contracts   │
│ and non-contract addresses.                                                                      │
└──────────────────────────────────────────────────────────────────────────────────────────────────┘"#
            .to_owned();

        Self {
            component: "general".to_owned(),
            error_code: None,
            formatted_message: message.clone(),
            message,
            severity: "warning".to_owned(),
            source_location: src.map(SourceLocation::from_str).and_then(Result::ok),
            r#type: "Warning".to_owned(),
        }
    }

    ///
    /// Returns the `origin` instruction usage warning.
    ///
    pub fn message_tx_origin(src: Option<&str>) -> Self {
        let message = r#"
┌──────────────────────────────────────────────────────────────────────────────────────────────────┐
│ Warning: You are checking for 'tx.origin' in your code, which might lead to unexpected behavior. │
│ zkSync Era comes with native account abstraction support, and therefore the initiator of a       │
│ transaction might be different from the contract calling your code. It is highly recommended NOT │
│ to rely on tx.origin, but use msg.sender instead.                                                │
│ Read more about Account Abstraction at https://v2-docs.zksync.io/dev/developer-guides/aa.html    │
└──────────────────────────────────────────────────────────────────────────────────────────────────┘"#
            .to_owned();

        Self {
            component: "general".to_owned(),
            error_code: None,
            formatted_message: message.clone(),
            message,
            severity: "warning".to_owned(),
            source_location: src.map(SourceLocation::from_str).and_then(Result::ok),
            r#type: "Warning".to_owned(),
        }
    }

    ///
    /// Returns the internal function pointer usage error.
    ///
    pub fn message_internal_function_pointer(src: Option<&str>) -> Self {
        let message = r#"
┌──────────────────────────────────────────────────────────────────────────────────────────────────┐
│ Error: Internal function pointers are not supported in EVM legacy assembly pipeline.             │
│ Please use the Yul IR codegen instead.                                                           │
└──────────────────────────────────────────────────────────────────────────────────────────────────┘"#
            .to_owned();

        Self {
            component: "general".to_owned(),
            error_code: None,
            formatted_message: message.clone(),
            message,
            severity: "error".to_owned(),
            source_location: src.map(SourceLocation::from_str).and_then(Result::ok),
            r#type: "Error".to_owned(),
        }
    }

    ///
    /// Appends the contract path to the message..
    ///
    pub fn push_contract_path(&mut self, path: &str) {
        self.formatted_message
            .push_str(format!("\n--> {path}\n").as_str());
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.formatted_message)
    }
}
