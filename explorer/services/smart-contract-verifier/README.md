# <h1 align="center"> Smart-contract Verifier </h1>

**Smart-contract verifier** is a service for verifying EVM-based contracts. The primary function of this service is to receive bytecode and potential source files as inputs and determine whether the files and the bytecode correspond to each other.

This service serves as the core component for all activities related to smart-contract verification in BlockScout. It is essential for enabling smart-contract verification functionality on your instance.

## Requirements
No additional dependencies

## How to enable
Set the following ENVs on blockscout instance:
- `MICROSERVICE_SC_VERIFIER_ENABLED=true`
- `MICROSERVICE_SC_VERIFIER_URL={service_url}`
- `MICROSERVICE_SC_VERIFIER_TYPE=sc_verifier`

## Envs
Here, we describe variables specific to this service. Variables common to all services can be found [here](../docs/common-envs.md).

[anchor]: <> (anchors.envs.start)

| Variable                                                       | Required | Description                                                             | Default value                                                                |
|----------------------------------------------------------------|----------|-------------------------------------------------------------------------|------------------------------------------------------------------------------|
| `SMART_CONTRACT_VERIFIER__SOLIDITY__ENABLED`                   |          | Enable Solidity verification endpoints                                  | `true`                                                                       |
| `SMART_CONTRACT_VERIFIER__SOLIDITY__FETCHER__LIST__LIST_URL`   |          | Url that contains a list available Solidity compilers                   | `https://binaries.soliditylang.org/linux-amd64/list.json`                    |
| `SMART_CONTRACT_VERIFIER__SOLIDITY__REFRESH_VERSIONS_SCHEDULE` |          | Cron-format schedule to update the list of available Solidity compilers | `0 0 * * * * *`                                                              |
| `SMART_CONTRACT_VERIFIER__SOLIDITY__COMPILERS_DIR`             |          | Directory where Solidity compilers will be downloaded                   | `/tmp/solidity-compilers`                                                    |
| `SMART_CONTRACT_VERIFIER__VYPER__ENABLED`                      |          | Enable Vyper verification endpoints                                     | `true`                                                                       |
| `SMART_CONTRACT_VERIFIER__VYPER__FETCHER__LIST__LIST_URL`      |          | Url that contains a list of available Vyper compilers                   | `https://raw.githubusercontent.com/blockscout/solc-bin/main/vyper.list.json` |
| `SMART_CONTRACT_VERIFIER__VYPER__REFRESH_VERSIONS_SCHEDULE`    |          | Cron-format schedule to update the list of available Vyper compilers    | `0 0 * * * * *`                                                              |
| `SMART_CONTRACT_VERIFIER__VYPER__COMPILERS_DIR`                |          | Directory where Vyper compilers will be downloaded                      | `/tmp/vyper-compilers`                                                       |
| `SMART_CONTRACT_VERIFIER__SOURCIFY__ENABLED`                   |          | Enable Soucify verification endpoint                                    | `true`                                                                       |
| `SMART_CONTRACT_VERIFIER__SOURCIFY__API_URL`                   |          | Sourcify API url                                                        | `https://sourcify.dev/server/`                                               |
| `SMART_CONTRACT_VERIFIER__SOURCIFY__VERIFICATION_ATTEMPTS`     |          | Number of attempts the server makes to Sourcify API. Must be at least 1 | `3`                                                                          |
| `SMART_CONTRACT_VERIFIER__SOURCIFY__REQUEST_TIMEOUT`           |          | Timeout in seconds for a single request to Sourcify API                 | `15`                                                                         |
| `SMART_CONTRACT_VERIFIER__COMPILERS__MAX_THREADS`              |          | Maximum number of concurrent compilations                               | `8`                                                                          |

[anchor]: <> (anchors.envs.end)

## Links
- Demo - https://http.sc-verifier.services.blockscout.com
- [Swagger](https://blockscout.github.io/swaggers/services/smart-contract-verifier/index.html)
- [Packages](https://github.com/blockscout/blockscout-rs/pkgs/container/smart-contract-verifier)
- [Releases](https://github.com/blockscout/blockscout-rs/releases?q=smart-contract-verifier&expanded=true)