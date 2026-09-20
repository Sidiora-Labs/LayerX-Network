import { expect } from 'chai';
import { bothProviders, rawPax, expectJsonRpcError, waitUntil } from '../utils/chainUtils';
import { readRuntimeState, RuntimeState, claimPool } from '../utils/testUtils';
import { EvmAccount } from '../utils/evmUtils';
import { FILTER_ID } from '../utils/logsUtils';
import { HASH32 } from '../utils/format';

describe('eth_newBlockFilter', function () {
    this.timeout(180 * 1000);

    const { pax, geth } = bothProviders();

    let runtime: RuntimeState;
    let producer: EvmAccount;

    before(async () => {
        runtime = readRuntimeState();
        [producer] = claimPool(runtime, pax, 1, 'eth_newBlockFilter');
    });

    describe('happy path / lifecycle', () => {
        it('returns a well-formed, opaque handle', async () => {
            const id = await pax.send('eth_newBlockFilter', []);
            expect(id, 'block filter id is opaque hex').to.match(FILTER_ID);
            await pax.send('eth_uninstallFilter', [id]);
        });

        it('hands out distinct ids for separate block filters', async () => {
            const [a, b] = await Promise.all([
                pax.send('eth_newBlockFilter', []),
                pax.send('eth_newBlockFilter', []),
            ]);
            expect(a).to.not.equal(b);
            await Promise.all([
                pax.send('eth_uninstallFilter', [a]),
                pax.send('eth_uninstallFilter', [b]),
            ]);
        });

        it('delivers canonical block hashes, including the block our tx landed in', async () => {
            const id = await pax.send('eth_newBlockFilter', []);
            const receipt = await (
                await producer.wallet.sendTransaction({ to: producer.address, value: 0 })
            ).wait();

            // eth_getFilterChanges returns only the hashes new since the previous poll, and Pax mines
            // continuously, so the first delta can be an unrelated earlier block while our tx's block
            // shows up on a later poll. Drain across polls until that block hash appears.
            const hashes: string[] = [];
            await waitUntil(
                async () => {
                    hashes.push(...(await pax.send('eth_getFilterChanges', [id])));
                    return hashes.includes(receipt!.blockHash) ? hashes : null;
                },
                { timeoutMs: 15_000, intervalMs: 300, label: 'block filter delivers our tx block hash' },
            );

            expect(hashes.length, 'at least one new block hash').to.be.greaterThan(0);
            hashes.forEach((h: string) => expect(h, 'block hash').to.match(HASH32));
            expect(hashes, 'includes the block our tx landed in').to.include(receipt!.blockHash);
            await pax.send('eth_uninstallFilter', [id]);
        });

        it('uninstalls once (true), then reports gone (false), then no longer resolves', async () => {
            const id = await pax.send('eth_newBlockFilter', []);
            expect(await pax.send('eth_uninstallFilter', [id]), 'first uninstall').to.equal(true);
            expect(await pax.send('eth_uninstallFilter', [id]), 'second uninstall').to.equal(false);
            const after = await rawPax('eth_getFilterChanges', [id]);
            expectJsonRpcError(after, -32000, /filter does not exist/);
        });
    });

    describe('schema matching (parity with geth)', () => {
        it('geth also returns an opaque hex handle', async () => {
            const id = await geth.send('eth_newBlockFilter', []);
            expect(id, 'geth block filter id').to.match(FILTER_ID);
            await geth.send('eth_uninstallFilter', [id]);
        });
    });

    describe('wrong params / error handling', () => {
        it('an unknown filter id fails with -32000 (filter does not exist)', async () => {
            const res = await rawPax('eth_getFilterChanges', ['0xdeadbeefdeadbeefdeadbeefdeadbeef']);
            expectJsonRpcError(res, -32000, /filter does not exist/);
        });
    });
});
