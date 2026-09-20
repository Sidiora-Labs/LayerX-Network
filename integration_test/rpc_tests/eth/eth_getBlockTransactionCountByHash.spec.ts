import { ethers } from 'ethers';
import { expect } from 'chai';
import { bothProviders, rawPax, rawGeth, expectJsonRpcError } from '../utils/chainUtils';
import { readRuntimeState, RuntimeState, claimPool, expectSameError } from '../utils/testUtils';
import { EvmAccount, fundFromUnlocked } from '../utils/evmUtils';
import { sharedRichBlock, sendSingleTx, RichBlock, SentTx, richFailedTxs, blockReceipts, txCountByHash, txCountByNumber, assertTxCount, findEmptyBlock } from '../utils/txUtils';

describe('eth_getBlockTransactionCountByHash', function () {
    this.timeout(300 * 1000);

    const { pax, geth } = bothProviders();

    let runtime: RuntimeState;
    let richPax: RichBlock;
    let paxOne: { number: number; hash: string; tx: SentTx };
    let gethOne: { number: number; hash: string; tx: SentTx };
    let gethSigner: EvmAccount;
    let emptyBlock: { number: number; hash: string } | undefined;

    before(async function () {
        this.timeout(300 * 1000);
        runtime = readRuntimeState();
        const [paxOneSigner] = claimPool(runtime, pax, 1, 'eth_getBlockTransactionCountByHash');

        const gethDev: string = (await geth.send('eth_accounts', []))[0];
        gethSigner = EvmAccount.fromPrivateKey(ethers.Wallet.createRandom().privateKey, geth);
        await fundFromUnlocked(geth, gethDev, gethSigner.address, ethers.parseEther('10'));

        richPax = await sharedRichBlock(pax, runtime);
        paxOne = await sendSingleTx(pax, paxOneSigner);
        gethOne = await sendSingleTx(geth, gethSigner);
        emptyBlock = await findEmptyBlock(pax);
    });

    describe('counts agree with every other view of the block', () => {
        it('counts every transaction in the rich block', async () => {
            const count = await txCountByHash(pax, richPax.hash);
            assertTxCount(count, richPax.txs.length, 'rich block tx count');
        });

        it('equals the eth_getBlockByHash tx list length and the receipt count', async () => {
            const [count, block, receipts] = await Promise.all([
                txCountByHash(pax, richPax.hash),
                pax.send('eth_getBlockByHash', [richPax.hash, false]),
                blockReceipts(pax, richPax.hash),
            ]);
            assertTxCount(count, block.transactions.length, 'count == block.transactions');
            expect(receipts.length, 'count == receipts length').to.equal(Number(BigInt(count)));
        });

        it('agrees with the by-number count for the same block', async () => {
            const [byHash, byNumber] = await Promise.all([
                txCountByHash(pax, richPax.hash),
                txCountByNumber(pax, richPax.number),
            ]);
            expect(byHash, 'byHash count == byNumber count').to.equal(byNumber);
        });

        it('includes the two included-but-failed txs in the count', async () => {
            const [count, block] = await Promise.all([
                txCountByHash(pax, richPax.hash),
                pax.send('eth_getBlockByHash', [richPax.hash, false]),
            ]);
            const { outOfGas, revertErc20 } = richFailedTxs(richPax);
            for (const sent of [outOfGas, revertErc20]) {
                expect(block.transactions, `${sent.kind} is counted in the block`).to.include(
                    sent.hash,
                );
            }
            assertTxCount(count, block.transactions.length, 'count includes the failed txs');
        });

        it('a known empty block reports 0x0', async function () {
            if (!emptyBlock) this.skip();
            const count = await txCountByHash(pax, emptyBlock!.hash);
            expect(count, 'empty block count is exactly 0x0').to.equal('0x0');
        });
    });

    describe('geth parity', () => {
        it('a single-transaction block counts 0x1 on both chains', async () => {
            const [s, g] = await Promise.all([
                txCountByHash(pax, paxOne.hash),
                txCountByHash(geth, gethOne.hash),
            ]);
            expect(s, 'Pax single-tx count').to.equal('0x1');
            expect(g, 'geth single-tx count').to.equal('0x1');
        });
    });

    describe('wrong params / error handling (parity with geth)', () => {
        it('empty params fail identically (-32602, missing argument 0)', async () => {
            const [s, g] = await Promise.all([
                rawPax('eth_getBlockTransactionCountByHash', []),
                rawGeth('eth_getBlockTransactionCountByHash', []),
            ]);
            expectJsonRpcError(s, -32602, /missing value for required argument 0/);
            expectSameError(s, g);
        });

        it('a wrong-length hash fails identically (-32602, common.Hash)', async () => {
            const [s, g] = await Promise.all([
                rawPax('eth_getBlockTransactionCountByHash', ['0x1234']),
                rawGeth('eth_getBlockTransactionCountByHash', ['0x1234']),
            ]);
            expectJsonRpcError(s, -32602, /hex string has length 4, want 64 for common\.Hash/);
            expectSameError(s, g);
        });

        it('an unknown block hash returns null on both chains', async () => {
            const unknown = '0x' + 'ab'.repeat(32);
            const [s, g] = await Promise.all([
                rawPax('eth_getBlockTransactionCountByHash', [unknown]),
                rawGeth('eth_getBlockTransactionCountByHash', [unknown]),
            ]);
            // An absent block has no count to report: both return a null result, not an error.
            expect(g.error, 'geth does not error').to.equal(undefined);
            expect(g.result, 'geth returns null for an unknown block').to.equal(null);
            expect(s.error, 'Pax does not error').to.equal(undefined);
            expect(s.result, 'Pax returns null for an unknown block').to.equal(null);
        });
    });
});
