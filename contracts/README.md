## Running hardhat tests locally
 * start up a local instance of pax: `./scripts/initialize_local_chain.sh`
 * run a hardhat tests:
    * `cd contracts`
    * `npx hardhat test --network paxlocal test/ERC20toCW20PointerTest.js`

## Compile and build contracts with Foundry
 * run from the repository root: `forge install` and `FOUNDRY_CONFIG=foundry.paxeer.toml forge build`
 * This will generate binaries and abis in the `contracts/out/` directory

## Updating Pointer contracts across codebase
 * Follow instructions above to compile and build the contracts
 * copy the binary under the corresponding `bytecode:object` into the pointer's `.bin` file under `modules/evm/artifacts/` (for example `modules/evm/artifacts/cw20/CW20ERC20Pointer.bin`)
 * restart paxd