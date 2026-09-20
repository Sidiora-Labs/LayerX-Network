import { expect } from 'chai';
import { bothProviders, isReachable, rawPax, rawGeth, rawAccountless, expectJsonRpcError } from '../utils/chainUtils';
import { ADDRESS, ADDRESS_LOWER } from '../utils/format';
import { Endpoints } from '../config/endpoints';

describe('eth_accounts Tests', function () {
    this.timeout(60 * 1000);

    const { pax, geth } = bothProviders();

    describe('Accounts queries', () => {
        it('returns a JSON array', async () => {
            const accounts = await pax.send('eth_accounts', []);
            expect(accounts).to.be.an('array');
        });

        it('every entry is a well-formed 20-byte address', async () => {
            const accounts: string[] = await pax.send('eth_accounts', []);
            for (const acct of accounts) {
                expect(acct, `account ${acct}`).to.match(ADDRESS);
            }
        });

        it('contains no duplicate addresses', async () => {
            const accounts: string[] = await pax.send('eth_accounts', []);
            const lower = accounts.map(a => a.toLowerCase());
            expect(new Set(lower).size).to.equal(lower.length);
        });

        it('returns the same set of accounts across repeated calls', async () => {
            // NOTE: Pax does not guarantee a stable *order* — it serializes the keyring
            // from a Go map, so the order varies call-to-call (geth, by contrast, returns
            // stable insertion order).
            const results: string[][] = await Promise.all(
                Array.from({ length: 4 }, () => pax.send('eth_accounts', [])),
            );
            const sortedSet = (a: string[]) => [...a].map(x => x.toLowerCase()).sort();
            const baseline = sortedSet(results[0]);
            for (const r of results) {
                expect(sortedSet(r)).to.deep.equal(baseline);
            }
        });
    });

    describe('schema matching', () => {
        it('Pax and geth both serialize addresses in lower-case (non-checksummed) form', async () => {
            const [paxAccounts, gethAccounts] = await Promise.all([
                pax.send('eth_accounts', []),
                geth.send('eth_accounts', []),
            ]);
            for (const acct of [...paxAccounts, ...gethAccounts]) {
                expect(acct, `account ${acct}`).to.match(ADDRESS_LOWER);
            }
        });
    });

    describe('empty / null handling', () => {
        it('a keyless node returns [] (empty array), never null', async function () {
            if (!(await isReachable(Endpoints.accountless))) {
                this.skip();
            }
            const body = await rawAccountless<string[]>('eth_accounts', []);
            expect(body.error, JSON.stringify(body.error)).to.equal(undefined);
            expect(body.result, 'keyless node must encode the empty set as []').to.deep.equal([]);
            expect(body.result).to.not.equal(null);
        });
    });

    describe('wrong params / error handling', () => {
        it('Pax rejects an extra positional parameter with -32602, identically to geth', async () => {
            const [paxBody, gethBody] = await Promise.all([
                rawPax('eth_accounts', ['latest']),
                rawGeth('eth_accounts', ['latest']),
            ]);
            expectJsonRpcError(paxBody, -32602, /too many arguments, want at most 0/i);
            expectJsonRpcError(gethBody, -32602, /too many arguments, want at most 0/i);
            expect(paxBody.error?.code).to.equal(gethBody.error?.code);
            expect(paxBody.error?.message).to.equal(gethBody.error?.message);
        });

        it('Pax rejects non-array params with -32602 non-array args, identically to geth', async () => {
            const [paxBody, gethBody] = await Promise.all([
                rawPax('eth_accounts', 'latest'),
                rawGeth('eth_accounts', 'latest'),
            ]);
            expectJsonRpcError(paxBody, -32602, /non-array args/i);
            expectJsonRpcError(gethBody, -32602, /non-array args/i);
            expect(paxBody.error?.code).to.equal(gethBody.error?.code);
            expect(paxBody.error?.message).to.equal(gethBody.error?.message);
        });
    });
});
