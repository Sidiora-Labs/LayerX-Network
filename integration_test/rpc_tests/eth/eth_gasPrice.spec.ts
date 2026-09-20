import { expect } from 'chai';
import { bothProviders, rawPax, rawGeth, expectJsonRpcError, blockGasInfo, Eip1559Params, queryEip1559Params, waitUntil } from '../utils/chainUtils';
import { readRuntimeState, RuntimeState, expectSameError, claimPool } from '../utils/testUtils';
import { EvmAccount } from '../utils/evmUtils';
import { burnGasBurst } from '../utils/txUtils';
import { HEX_QUANTITY } from '../utils/format';
import { gasPrice, gasPriceAtStableBlock, assertPaxGasPriceTracks } from '../utils/gasPriceUtils';

describe('eth_gasPrice', function () {
    this.timeout(240 * 1000);

    const { pax, geth } = bothProviders();

    let runtime: RuntimeState;
    let spammers: EvmAccount[];
    let paxParams: Eip1559Params | null;
    let floorBase: bigint;
    let floorGasPrice: bigint;

    before(async () => {
        runtime = readRuntimeState();
        spammers = claimPool(runtime, pax, 5, 'eth_gasPrice');
        paxParams = await queryEip1559Params();
        floorBase = paxParams ? BigInt(paxParams.minFeePerGas) : 1_000_000_000n;
        floorGasPrice = (floorBase * 110n) / 100n;
    });

    describe('happy path / schema', () => {
        it('returns a positive canonical quantity on Pax', async () => {
            const body = await rawPax<string>('eth_gasPrice', []);
            expect(body.error, JSON.stringify(body.error)).to.equal(undefined);
            expect(body.result).to.match(HEX_QUANTITY);
            expect(BigInt(body.result!) > 0n).to.equal(true);
        });

        it('returns a positive canonical quantity on geth', async () => {
            const body = await rawGeth<string>('eth_gasPrice', []);
            expect(body.error, JSON.stringify(body.error)).to.equal(undefined);
            expect(body.result).to.match(HEX_QUANTITY);
            expect(BigInt(body.result!) > 0n).to.equal(true);
        });

        it('is at least the current base fee, so a tx priced at it is includable (both)', async () => {
            const [sGas, sBlk, gGas, gBlk] = await Promise.all([
                gasPrice(pax),
                blockGasInfo(pax, 'latest'),
                gasPrice(geth),
                blockGasInfo(geth, 'latest'),
            ]);
            expect(sGas >= sBlk.baseFee, `pax gasPrice ${sGas} < base ${sBlk.baseFee}`).to.equal(true);
            expect(gGas >= gBlk.baseFee, `geth gasPrice ${gGas} < base ${gBlk.baseFee}`).to.equal(true);
        });
    });

    describe('relationship to base fee and priority fee', () => {
        it('[Pax] an uncongested gas price is exactly the base fee plus the 10% buffer', async () => {
            const { gasPrice: price, block } = await gasPriceAtStableBlock(pax);
            await assertPaxGasPriceTracks(pax, price, block);
        });

        it('[geth] the gas price equals the base fee plus the suggested priority fee (exact)', async () => {
            const [price, tip, blk] = await Promise.all([
                gasPrice(geth),
                BigInt(await geth.send('eth_maxPriorityFeePerGas', [])),
                blockGasInfo(geth, 'latest'),
            ]);
            expect(price, 'geth gasPrice = baseFee + tip').to.equal(blk.baseFee + tip);
        });

        it('[Pax] maxPriorityFeePerGas defaults to 1 gwei while the chain is uncongested', async () => {
            // Quiescent runs sit well under the 80% congestion threshold.
            const tip = BigInt(await pax.send('eth_maxPriorityFeePerGas', []));
            expect(tip).to.equal(1_000_000_000n);
        });
    });

    describe('reflects base fee increases (Pax)', () => {
        it('gas price rises with the base fee and keeps tracking nextBaseFee * 1.1', async function () {
            const samples: { gasPrice: bigint; block: number }[] = [];
            const { maxBlock } = await burnGasBurst(pax, runtime, spammers, {
                rounds: 10,
                onRound: async () => {
                    samples.push(await gasPriceAtStableBlock(pax));
                },
            });
            const peakGasPrice = samples.reduce((m, s) => (s.gasPrice > m ? s.gasPrice : m), 0n);

            expect(
                peakGasPrice > floorGasPrice,
                `gas price should rise above the idle ${floorGasPrice}`,
            ).to.equal(true);

            // Every reading must track the live base fee through formula.
            for (const s of samples) {
                await assertPaxGasPriceTracks(pax, s.gasPrice, s.block);
            }
        });

        it('gas price decays once the load stops', async function () {
            const start = await gasPrice(pax);
            if (start <= floorGasPrice) this.skip();
            let lower: bigint | null = null;
            lower = await waitUntil<bigint>(
                async () => {
                    const now = await gasPrice(pax);
                    return now < start ? now : null;
                },
                { timeoutMs: 60_000, intervalMs: 500, label: 'gas price decays' },
            );
            
            expect(lower! < start, `gas price ${lower} should drop below post-burst ${start}`).to.equal(true);
            expect(lower! >= floorGasPrice, `gas price ${lower} never falls below the floor`).to.equal(true);
        });
    });

    describe('wrong params / error handling', () => {
        it('an extra positional argument fails identically (-32602, want at most 0)', async () => {
            const [s, g] = await Promise.all([
                rawPax('eth_gasPrice', ['latest']),
                rawGeth('eth_gasPrice', ['latest']),
            ]);
            expectJsonRpcError(s, -32602, /too many arguments, want at most 0/);
            expectSameError(s, g);
        });

        it('an object argument fails identically (-32602, want at most 0)', async () => {
            const [s, g] = await Promise.all([
                rawPax('eth_gasPrice', [{}]),
                rawGeth('eth_gasPrice', [{}]),
            ]);
            expectJsonRpcError(s, -32602, /too many arguments, want at most 0/);
            expectSameError(s, g);
        });
    });
});
