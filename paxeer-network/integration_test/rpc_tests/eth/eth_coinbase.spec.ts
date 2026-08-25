import { ethers } from "ethers";
import { expect } from "chai";
import { fromBech32 } from "@cosmjs/encoding";

import { paxRpc } from "../utils/chainUtils";
import { readRuntimeState } from "../utils/testUtils";
import { claimPool } from "../utils/testUtils";
import { isPaxDocker, paxAddressFromMnemonic, feeCollectorCosmosAddress } from "../utils/cosmosUtils";
import { bankBalanceUhpx } from "../utils/cosmosUtils";
import { rawPax, rawGeth, expectJsonRpcError } from "../utils/chainUtils";
import { WEI_PER_UHPX, ZERO_ADDRESS } from "../utils/constants";

describe('eth_coinbase Tests', function () {
    this.timeout(120 * 1000);

    let paxProvider: ethers.JsonRpcProvider;
    let feeCollectorAddr: string;

    before(async () => {
        paxProvider = paxRpc();
        const { adminMnemonic } = readRuntimeState().funded;
        const { prefix } = fromBech32(await paxAddressFromMnemonic(adminMnemonic));
        feeCollectorAddr = feeCollectorCosmosAddress(prefix);
    });

    it('eth_coinbase returns a syntactically valid 20-byte EVM address', async () => {
        const coinbase = await paxProvider.send('eth_coinbase', []);
        expect(coinbase).to.match(/^0x[0-9a-fA-F]{40}$/);
        expect(coinbase.toLowerCase()).to.not.equal(ZERO_ADDRESS);
    });

    it('eth_coinbase is distinct from block.coinbase (the per-block proposer)', async () => {
        const [coinbase, block] = await Promise.all([
            paxProvider.send('eth_coinbase', []),
            paxProvider.send('eth_getBlockByNumber', ['latest', false]),
        ]);
        expect(block.miner).to.match(/^0x[0-9a-fA-F]{40}$/);
        expect(coinbase.toLowerCase()).to.not.equal(block.miner.toLowerCase());
    });

    it('eth_coinbase equals the EVM-mapped address of the cosmos fee_collector module account', async () => {
        const coinbase = (await paxProvider.send('eth_coinbase', [])).toLowerCase();

        const evmAddress: string = await paxProvider.send('pax_getEVMAddress', [feeCollectorAddr]);
        expect(evmAddress.toLowerCase()).to.equal(coinbase);
    });

    it('eth_coinbase round-trips: pax_getPaxAddress(coinbase) equals the derived fee_collector address', async () => {
        const coinbase = await paxProvider.send('eth_coinbase', []);

        const paxAddress: string = await paxProvider.send('pax_getPaxAddress', [coinbase]);
        expect(paxAddress).to.equal(feeCollectorAddr);
    });

    it('EVM tx fees accrue to eth_coinbase (the fee_collector) and are swept each block', async function () {
        if (!(await isPaxDocker())) this.skip();

        const coinbase = (await paxProvider.send('eth_coinbase', [])).toLowerCase();
        const [signer] = claimPool(readRuntimeState(), paxProvider, 1, 'eth_coinbase');

        const gasPrice = BigInt(await paxProvider.send('eth_gasPrice', []));
        const tip = ethers.parseUnits('2', 'gwei');
        const tx = await signer.wallet.sendTransaction({
            to: signer.address,
            value: 0n,
            maxFeePerGas: gasPrice * 3n + tip,
            maxPriorityFeePerGas: tip,
        });
        const receipt = await tx.wait(1, 30_000);
        const blockN = receipt!.blockNumber;
        const ourFeeWei = receipt!.gasUsed * receipt!.gasPrice!;

        const balN = await bankBalanceUhpx(feeCollectorAddr, blockN);
        expect(balN * WEI_PER_UHPX >= ourFeeWei).to.equal(
            true,
            `fee_collector at height ${blockN} (${balN} uhpx) must include our ${ourFeeWei} wei fee`,
        );
    });

    it('rejects extra parameters on Pax with -32602 and geths exact message', async () => {
        const [positional, object] = await Promise.all([
            rawPax('eth_coinbase', ['latest']),
            rawPax('eth_coinbase', [{}]),
        ]);
        expectJsonRpcError(positional, -32602, /^too many arguments, want at most 0$/);
        expectJsonRpcError(object, -32602, /^too many arguments, want at most 0$/);
    });
});
