const { fundAddress, fundPaxAddress, getPaxBalance, associateKeyStrict, importKey, waitForReceipt, bankSend, evmSend, getNativeAccount, execute, getKeyPaxAddress, getAccountSequence, waitForCondition} = require("./lib");
const { expect } = require("chai");

describe("Associate Balances", function () {

    const keys = {
        "test1": {
            paxAddress: 'pax1nzdg7e6rvkrmvp5zzmp5tupuj0088nqsvkhvvu',
            evmAddress: '0x90684e7F229f2d8E2336661f79caB693E4228Ff7'
        },
        "test2": {
            paxAddress: 'pax1jqgph9jpdtvv64e3rzegxtssvgmh7lxnzyq4cf',
            evmAddress: '0x28b2B0621f76A2D08A9e04acb7F445E61ba5b7E7'
        },
        "test3": {
            paxAddress: 'pax1qkawqt7dw09rkvn53lm2deamtfcpuq9v75kvf2',
            evmAddress: '0xCb2FB25A6a34Ca874171Ac0406d05A49BC45a1cF',
            castAddress: 'pax1evhmykn2xn9gwst34szqd5z6fx7ytgw0waypee',
        }
    }

    const addresses = {
        paxAddress: 'pax1nzdg7e6rvkrmvp5zzmp5tupuj0088nqsvkhvvu',
        evmAddress: '0x90684e7F229f2d8E2336661f79caB693E4228Ff7'
    }

    function truncate(num, byThisManyDecimals) {
        return parseFloat(`${num}`.slice(0, 12))
    }

    async function verifyAssociation(paxAddr, evmAddr, associateFunc) {
        const multiplier = BigInt(1000000000000)
        const beforePax = BigInt(await getPaxBalance(paxAddr))
        const beforeEvm = await ethers.provider.getBalance(evmAddr)
        const gas = await associateFunc(paxAddr)
        const expectedEvm = (beforePax * multiplier) + beforeEvm - (gas * multiplier)
        await waitForCondition(
            async () => (await ethers.provider.getBalance(evmAddr)) === expectedEvm,
            `EVM balance of ${evmAddr} to equal ${expectedEvm}`,
        )
        const afterPax = BigInt(await getPaxBalance(paxAddr))
        const afterEvm = await ethers.provider.getBalance(evmAddr)

        console.log(`PAX Balance (before): ${beforePax}`)
        console.log(`EVM Balance (before): ${beforeEvm}`)
        console.log(`PAX Balance (after): ${afterPax}`)
        console.log(`EVM Balance (after): ${afterEvm}`)

        expect(afterEvm).to.equal(expectedEvm)
        expect(afterPax).to.equal(truncate(beforePax - gas))
    }

    before(async function(){
        await importKey("test1", "../contracts/test/test1.key")
        await importKey("test2", "../contracts/test/test2.key")
        await importKey("test3", "../contracts/test/test3.key")
    })

    it("should associate with pax transaction", async function(){
        const addr = keys.test1
        await fundPaxAddress(addr.paxAddress, "10000000000")
        await fundAddress(addr.evmAddress, "200");

        await verifyAssociation(addr.paxAddress, addr.evmAddress, async function(){
            await bankSend(addr.paxAddress, "test1")
            return BigInt(20000)
        })
    });

    it("should associate with evm transaction", async function(){
        const addr = keys.test2
        await fundPaxAddress(addr.paxAddress, "10000000000")
        await fundAddress(addr.evmAddress, "200");

        await verifyAssociation(addr.paxAddress, addr.evmAddress, async function(){
            const txHash = await evmSend(addr.evmAddress, "test2", "0")
            const receipt = await waitForReceipt(txHash)
            return BigInt(receipt.gasUsed * (receipt.gasPrice / BigInt(1000000000000)))
        })
    });

    it("should associate with associate transaction", async function(){
        const addr = keys.test3
        await fundPaxAddress(addr.paxAddress, "10000000000")
        await fundAddress(addr.evmAddress, "200");

        await verifyAssociation(addr.paxAddress, addr.evmAddress, async function(){
            await associateKeyStrict("test3")
            return BigInt(0)
        });

        // it should not be able to send funds to the cast address after association
        expect(await getPaxBalance(addr.castAddress)).to.equal(0);
        // fundPaxAddress would deadlock here: its wait condition is "recipient
        // balance reaches target", which is exactly what this test asserts
        // should NOT happen (post-association routing blocks crediting the
        // cast address). Wait for admin's sequence to advance instead — the
        // causal "tx committed" signal that's independent of the side effect.
        const adminAddr = await getKeyPaxAddress("admin")
        const seqBefore = await getAccountSequence(adminAddr)
        await execute(`paxd tx bank send admin ${addr.castAddress} 100uhpx -b sync -y --fees 20000uhpx`)
        await waitForCondition(
            async () => (await getAccountSequence(adminAddr)) > seqBefore,
            `admin sequence > ${seqBefore}`,
        )
        expect(await getPaxBalance(addr.castAddress)).to.equal(0);
    });

})
