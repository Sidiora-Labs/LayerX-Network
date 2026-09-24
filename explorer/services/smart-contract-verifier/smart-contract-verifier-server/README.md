# <h1 align="center"> Smart-contract Verifier (Server) </h1>

Smart-contract verification service. Runs HTTP and (or) GRPC server and allows
making verification requests through corresponding APIs. The server itself is stateless
and answers requests based on provided information only.

## Configuration

Service supports configuration via configuration file and environment variables.
The latter overwrites the former in case if both are provided. For all missing fields
default values are used (if possible).

### Configuration file

Service uses a configuration file the path to which is specified via `SMART_CONTRACT_VERIFIER__CONFIG=[path]` environment variable.
The base configuration file with all available options could be found at [config/base.toml](./config/base.toml).

Below is an example of a simple configuration file which is filled with default values.

```toml
[server.http]
# When disabled, HTTP server is not running
enabled = true
# IP address and port number the HTTP server should listen to
addr = "0.0.0.0:8050"
# The maximum JSON payload is able to be processed
max_body_size = 2097152

[server.grpc]
# (Disabled by default) When disabled, GRPC server is not running
enabled = false
# IP address and port number the GRPC server should listen to
addr = "0.0.0.0:8051"

[solidity]
# When disabled, solidity related handlers are not available
enabled = true
# A directory where compilers would be downloaded to
compilers_dir = "/tmp/solidity-compilers"
# List of available solidity versions updates cron formatted schedule 
refresh_versions_schedule = "0 0 * * * * *"

[solidity.fetcher.list]
# List of all available solidity compilers and information about them.
list_url = "https://binaries.soliditylang.org/linux-amd64/list.json"

[vyper]
# When disabled, vyper related handlers are not available
enabled = true
# A directory where vyper compilers would be downloaded to
compilers_dir = "/tmp/vyper-compilers"
# List of available versions updates cron formatted schedule
refresh_versions_schedule = "0 0 * * * * *"

[vyper.fetcher.list]
# List of all available vyper compilers and information about them
list_url = "https://raw.githubusercontent.com/blockscout/solc-bin/main/vyper.list.json"

[sourcify]
# When disabled, sourcify related handlers are not available
enabled = true
# Sourcify API endpoint
api_url = "https://sourcify.dev/server/"
# Number of failing attempts the server makes to Sourcify API
verification_attempts = 3
# The maximum period (in seconds) the service is waiting for the Sourcify response
request_timeout = 10

[compilers]
# Maximum number of concurrent compilations. If omitted, number of CPU cores would be used
max_threads = 8

[metrics]
# When disabled, metrics are not available
enabled = false
# IP address and port number metrics related endpoint should listen to
addr = "0.0.0.0:6060"
# A route at which metrics related endpoint is available
route = "/metrics"

[jaeger]
# When disabled, jaeger tracing is not available
enabled = false
# An endpoint where jaeger collects all traces
agent_endpoint = "localhost:6831"
```

### Environment variables

Besides configuration file, one could use environment variables
to configure the service. If case of overlapping, those values
overwrite values from configuration file.
Variables have a hierarchical nature which
corresponds to the hierarchy in configuration file.
Double underscore (`__`) is used as a separator. All variables should use
`SMART_CONTRACT_VERIFIER` as a prefix.

All available options for configuration through environment variables could be found at
[config/base.env](./config/base.env)

# Grpc Api

Grpc description of available methods could be found in [proto](../smart-contract-verifier-proto/proto/v2). 

# Http Api

Swagger description is available in [swagger](../smart-contract-verifier-proto/swagger/v2/smart-contract-verifier.swagger.yaml).  

## Solidity Multi-Part files

### Route
`POST /api/v2/verifier/solidity/sources:verify-multi-part`

### Input

```json5
{
  // Bytecode to compare local compilation result with
  "bytecode": "0x608060...0033000b0c",
  // Either "CREATION_INPUT" or "DEPLOYED_BYTECODE", depending on what should be verified
  "bytecodeType": "CREATION_INPUT",
  // Compiler version used to compile the contract
  "compilerVersion": "v0.8.14+commit.80d49f37",
  // (optional) Version of the EVM to compile for. 
  // If absent results in default EVM version
  "evmVersion":  "default",
  // (optional) If present, optimizations are enabled with specified number of runs,
  // otherwise optimizations are disabled
  "optimizationRuns": 200,
  // Map from a source file name to the actual source code
  "sourceFiles": {
    "A.sol": "pragma solidity ^0.8.14; contract A {}",
    "B.sol": "pragma solidity ^0.8.14; contract B {}"
  },
  // Map from a library name to its address
  "libraries": {
    "MyLib": "0x123123..."
  }
}
```

## Solidity Standard-JSON input

### Route
`POST /api/v2/verifier/solidity/sources:verify-standard-json`

### Input
```json5
{
  // Bytecode to compare local compilation result with
  "bytecode": "0x608060...0033000b0c",
  // Either "CREATION_INPUT" or "DEPLOYED_BYTECODE", depending on what should be verified
  "bytecodeType": "CREATION_INPUT",
  // Compiler version used to compile the contract
  "compilerVersion": "v0.8.14+commit.80d49f37",
  // https://docs.soliditylang.org/en/latest/using-the-compiler.html#input-description
  "input": "{\"language\": \"Solidity\",\"sources\": { ... }, \"settings\": { ... }}"
}
```

## Vyper Multi-Part files

### Route
`POST /api/v2/verifier/vyper/sources:verify-multi-part`

### Input
```json5
{
  // Bytecode to compare local compilation result with
  "bytecode": "0x608060...0033000b0c",
  // Either "CREATION_INPUT" or "DEPLOYED_BYTECODE", depending on what should be verified
  "bytecodeType": "CREATION_INPUT",
  // Compiler version used to compile the contract
  "compilerVersion": "0.3.6+commit.4a2124d0",
  // (optional) Version of the EVM to compile for. 
  // If absent results in default EVM version
  "evmVersion":  "istanbul",
  // (optional) Flag enabling optimizations. If absent, default value is `true`
  "optimizations": "true",
  // Source file name to the actual source code
  "sourceFiles": {
    "A.vy": "# @version ^0.3.6\r\n\r\nuserName: public(String[100])\r\n\r\n@external\r\ndef __init__(name: String[100]):\r\n    self.userName = name\r\n\r\n@view\r\n@external\r\ndef getUserName() -> String[100]:\r\n    return self.userName\r\n"
  }
}
```

## Sourcify
Proxies verification requests to Sourcify service and returns responses (https://docs.sourcify.dev/docs/api/server/v1/verify/).

### Route
`POST /api/v2/verifier/sourcify/sources:verify`

### Input
```json5
{
  // Address of the contract to be verified 
  "address": "0xcafecafecafecafecafecafecafecafecafecafe",
  // The chain (network) the contract was deployed to 
  // (https://docs.sourcify.dev/docs/api/chains/)
  "chain": "100",
  // Files required for verification (see Sourcify Api)
  "files": {
    "A.sol": "pragma solidity ^0.8.14; contract A {}",
    "B.sol": "pragma solidity ^0.8.14; contract B {}",
    // https://docs.soliditylang.org/en/v0.8.14/metadata.html
    "metadata.json": "{ ... }"
  },
  // (optional) see Sourcify Api
  "chosenContract": 1
}
```

## Outputs
All verification requests have the same response format.

### Success
If verification succeeds, the service returns 200 with a success status:
```json5
{
  "message": "OK",
  "status": "SUCCESS", 
  "source": {
    // The name of the file verified contract was located at
    "fileName":  "A.sol",
    // The name of the contract which was verified
    "contractName": "A",
    // Compiler version used to compile the contract
    "compilerVersion": "v0.8.14+commit.80d49f37",
    // 'settings' key in Standard Input JSON
    // (https://docs.soliditylang.org/en/latest/using-the-compiler.html#input-description)
    "compilerSettings": "{ ... }",
    // One of "SOLIDITY", "VYPER", or "YUL". 
    // "SOURCE_TYPE_UNSPECIFIED" is also an option, but should be considered invalid by the clients. 
    "sourceType": "SOLIDITY",
    "sourceFiles": {
      "A.sol": "pragma solidity ^0.8.14; contract A {}",
      "B.sol": "pragma solidity ^0.8.14; contract B {}" 
    },
    // (optional) Contract abi (https://docs.soliditylang.org/en/latest/abi-spec.html?highlight=abi#json);
    // (does not exist for Yul contracts)
    "abi":  "[ { ... } ]";
    // (optional) Constructor arguments used for deploying verified contract
    "constructorArguments": "0xcafecafecafe",
    // Either "PARTIAL" or "FULL".
    // Similar to Sourcify (see https://docs.sourcify.dev/docs/full-vs-partial-match/)
    "matchType": "PARTIAL",
  },
  "extraData": {
    // Creation transaction input resultant from local compilation
    // parsed and split on Main and Meta parts. 
    // Is empty for Sourcify verification.
    "localCreationInputParts": [
      { "type": "main", "data": "0x1234.." },
      { "type": "meta", "data": "0xcafe.." }
    ],
    // Deployed bytecode resultant from local compilation
    // parsed and split on Main and Meta parts. 
    // Is empty for Sourcify verification.
    "localDeployedBytecodeParts": [
      { "type": "main", "data": "0x1234.." },
      { "type": "meta", "data": "0xcafe.." }
    ]
  }
}
```

### Verification Failure
If verification fails because of invalid verification data provided to it from outside,
the service returns 200 with the failure status:
```json5
{
  // Message indicating the reason for failure
  "message": "Compilation error: contracts/3_Ballot.sol:4:1: ParserError: Expected pragma, import directive or contract/interface/library/struct/enum/constant/function definition.\n12312313vddfvfdvfd\n^------^",
  // Non "SUCCESS" statuses indicate errors (currently only "FAILURE" is possible)
  "status": "FAILURE"
}
```

### Bad Request
There are data whose validity the requester is responsible to ensure.
That includes the bytecode to be a valid not-empty hex, the bytecode type to be
either "CREATION_INPUT" or "DEPLOYED_BYTECODE", and the compiler version to be valid.

In case any of that arguments are invalid, the service return 400 BadRequest error,
indicating that something is wrong with the caller.

## Version List

### Route
`GET /api/v2/verifier/solidity/versions`

### Input
No input required

### Output

```json5
{
  // List of all available versions in descending order
  "compilerVersions": ["0.8.15-nightly.2022.5.27+commit.095cc647","0.8.15-nightly.2022.5.25+commit.fdc3c8ee",..]
}
```

### Route
`GET /api/v2/verifier/vyper/versions`

### Input
No input required

### Output

```json5
{
  // List of all available versions in descending order
  "compilerVersions": ["v0.3.6+commit.4a2124d0","v0.3.4+commit.f31f0ec4",..]
}
```

# Compiler Settings (transition)
In the previous version the verifier partially parsed compiler settings and explicitly returned some of its values.
That included `evm_version`, `optimization`, `optimization_runs`, and `contract_libraries`. 
In the new version those keys have been removed, as all of them could be obtained by the client from `compiler_settings`. 
In this section we will describe how the client can parse that value.

Below is the description of settings from [Solidity docs](https://docs.soliditylang.org/en/v0.8.17/using-the-compiler.html#input-description) with the most related parts:
```json5
{
  // Optional: Optimizer settings
  "optimizer": {
    // Disabled by default.
    // NOTE: enabled=false still leaves some optimizations on. See comments below.
    // WARNING: Before version 0.8.6 omitting the 'enabled' key was not equivalent to setting
    // it to false and would actually disable all the optimizations.
    "enabled": true,
    // Optimize for how many times you intend to run the code.
    // Lower values will optimize more for initial deployment cost, higher
    // values will optimize more for high-frequency usage.
    "runs": 200,
  },
  // Version of the EVM to compile for.
  // Affects type checking and code generation. Can be homestead,
  // tangerineWhistle, spuriousDragon, byzantium, constantinople, petersburg, istanbul or berlin
  "evmVersion": "byzantium",
  // Addresses of the libraries. If not all libraries are given here,
  // it can result in unlinked objects whose output data is different.
  "libraries": {
    // The top level key is the name of the source file where the library is used.
    // If remappings are used, this source file should match the global path
    // after remappings were applied.
    // If this key is an empty string, that refers to a global level.
    "myFile.sol": {
      "MyLib": "0x123123..."
    }
  },
}
```

The process of parsing consists of trying to obtain required keys, and if they are missing using the default values instead:
1. `optimization` - either `compilerSettings[optimizer][enabled]`; or `null`, if any key is missed.
2. `optimization_runs` - if `optimization=true`, then either `compilerSettings[optimizer][runs]` (if exists) or `200`. Otherwise, `null`. 
3. `evm_version` - either `compilerSettings[evmVersion]` (if exists) or `default`.
4. `contract_libraries` - `compilerSettings[libraries]` (if exists) or `{}`.
