import { expect } from 'chai';
import { bothProviders, rawPax, rawGeth, expectJsonRpcError } from '../utils/chainUtils';
import { readRuntimeState, RuntimeState } from '../utils/testUtils';
import { sharedRichBlock, richFailedTxs, getVmError, RichBlock } from '../utils/txUtils';

const UNKNOWN_HASH = '0x' + '11'.repeat(32);

describe('eth_getVMError', function () {
    this.timeout(180 * 1000);

    const { pax, geth } = bothProviders();

    let runtime: RuntimeState;
    let rich: RichBlock;

    before(async () => {
        runtime = readRuntimeState();
        rich = await sharedRichBlock(pax, runtime);
    });

    describe('queries', () => {
        it('returns an empty string for a successful transaction', async () => {
            const ok = rich.txs.find(t => t.status === 1)!;
            expect(await getVmError(pax, ok.hash), 'a successful tx has no VM error').to.equal('');
        });

        it('reports "out of gas" for the included out-of-gas transaction', async () => {
            const { outOfGas } = richFailedTxs(rich);
            const vmErr = await getVmError(pax, outOfGas.hash);
            expect(vmErr, 'non-empty VM error').to.have.length.greaterThan(0);
            expect(vmErr, 'out-of-gas signature').to.match(/gas/i);
        });

        it('reports a revert for the included ERC20 transfer that reverts', async () => {
            const { revertErc20 } = richFailedTxs(rich);
            const vmErr = await getVmError(pax, revertErc20.hash);
            expect(vmErr, 'non-empty VM error').to.have.length.greaterThan(0);
            expect(vmErr, 'revert signature').to.match(/revert/i);
        });

        it('a successful and a failed tx are distinguishable by their VM error', async () => {
            const ok = rich.txs.find(t => t.status === 1)!;
            const { revertErc20 } = richFailedTxs(rich);
            const [okErr, badErr] = await Promise.all([
                getVmError(pax, ok.hash),
                getVmError(pax, revertErc20.hash),
            ]);
            expect(okErr).to.equal('');
            expect(badErr).to.not.equal('');
        });
    });

    describe('wrong params / error handling', () => {
        it('an unknown tx hash fails (no receipt to read a VM error from)', async () => {
            const res = await rawPax('eth_getVMError', [UNKNOWN_HASH]);
            expect(res.error, JSON.stringify(res)).to.not.equal(undefined);
        });

        it('a missing hash argument is rejected with -32602', async () => {
            const res = await rawPax('eth_getVMError', []);
            expectJsonRpcError(res, -32602, /missing value for required argument 0/);
        });
    });
});
