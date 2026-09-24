#[cfg(target_os = "linux")]
pub const DEFAULT_SOLIDITY_COMPILER_LIST: &str =
    "https://binaries.soliditylang.org/linux-amd64/list.json";
#[cfg(target_os = "macos")]
pub const DEFAULT_SOLIDITY_COMPILER_LIST: &str =
    "https://binaries.soliditylang.org/macosx-amd64/list.json";

#[cfg(target_os = "linux")]
pub const DEFAULT_VYPER_COMPILER_LIST: &str =
    "https://raw.githubusercontent.com/blockscout/solc-bin/main/vyper.list.json";
#[cfg(target_os = "macos")]
pub const DEFAULT_VYPER_COMPILER_LIST: &str =
    "https://raw.githubusercontent.com/blockscout/solc-bin/main/vyper.macos.list.json";

pub const DEFAULT_SOURCIFY_HOST: &str = "https://sourcify.dev/server/";

#[cfg(target_os = "linux")]
pub const DEFAULT_ZKSOLC_COMPILER_LIST: &str =
    "https://raw.githubusercontent.com/blockscout/solc-bin/main/zksolc.linux-amd64.list.json";
#[cfg(target_os = "macos")]
pub const DEFAULT_ZKSOLC_COMPILER_LIST: &str =
    "https://raw.githubusercontent.com/blockscout/solc-bin/main/zksolc.macosx-arm64.list.json";

#[cfg(target_os = "linux")]
pub const DEFAULT_ERA_SOLIDITY_COMPILER_LIST: &str =
    "https://raw.githubusercontent.com/blockscout/solc-bin/main/era-solidity.linux-amd64.list.json";
#[cfg(target_os = "macos")]
pub const DEFAULT_ERA_SOLIDITY_COMPILER_LIST: &str =
    "https://raw.githubusercontent.com/blockscout/solc-bin/main/era-solidity.macosx-arm64.list.json";
