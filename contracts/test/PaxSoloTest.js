const { expect } = require("chai");

const { setupSigners, getAdmin, getPaxBalance, printClaimMsg, addKey, getKeyPaxAddress, fundPaxAddress,hex2uint8,
    deployWasm, printClaimSpecificMsg, WASM, queryWasm, printClaimMsgBySender
} = require("./lib");


describe("Pax Solo Tester", function () {

    let accounts;
    let admin;

    before(async function () {
        accounts = await setupSigners(await hre.ethers.getSigners());
        admin = await getAdmin();
    })

    describe("Claim Tester", function () {
        const SoloPrecompileContract = '0x000000000000000000000000000000000000100C';
        let solo;
        let soloAddr;

        before(async function () {
            const signer = accounts[0].signer
            const contractABIPath = '../../precompiles/solo/abi.json';
            const contractABI = require(contractABIPath);
            // Get a contract instance
            solo = new ethers.Contract(SoloPrecompileContract, contractABI, signer);
            // setup account to be claimed
            await addKey("solo");
            soloAddr = await getKeyPaxAddress("solo");
            await fundPaxAddress(soloAddr);
        });

        it("Claims all tokens from the old account", async function () {
            let signerAddr = await accounts[0].signer.getAddress();
            let claimMsg = await printClaimMsg("solo", signerAddr);
            let payload = hex2uint8(claimMsg);
            const claim = await solo.claim(payload, {gasLimit: 100000});
            const receipt = await claim.wait();
            expect(receipt.status).to.equal(1);
            let postClaimBalance = await getPaxBalance(soloAddr);
            expect(postClaimBalance).to.equal(0);
        });
    });

    describe("Claim Imposter Tester", function () {
        const SoloPrecompileContract = '0x000000000000000000000000000000000000100C';
        let solo;
        let victimAddr;
        let imposterAddr;

        before(async function () {
            const signer = accounts[0].signer
            const contractABIPath = '../../precompiles/solo/abi.json';
            const contractABI = require(contractABIPath);
            // Get a contract instance
            solo = new ethers.Contract(SoloPrecompileContract, contractABI, signer);
            // setup account to be attacked
            await addKey("victim");
            victimAddr = await getKeyPaxAddress("victim");
            await fundPaxAddress(victimAddr);
            // setup imposter account
            await addKey("imposter");
            imposterAddr = await getKeyPaxAddress("imposter");
            await fundPaxAddress(imposterAddr);
        });

        it("Should not allow imposter to claim", async function () {
            let signerAddr = await accounts[0].signer.getAddress();
            let claimMsg = await printClaimMsgBySender("imposter", signerAddr, victimAddr);
            let payload = hex2uint8(claimMsg);
            try {
                const claim = await solo.claim(payload, {gasLimit: 100000});
                const receipt = await claim.wait();
            } catch (error) {
                expect(error.receipt.status).to.equal(0);
            }
        });
    });

    describe("Claim CW20 Tester", function () {
        const SoloPrecompileContract = '0x000000000000000000000000000000000000100C';
        let solo;
        let soloAddr;
        let cw20Address;

        before(async function () {
            const signer = accounts[0].signer
            const contractABIPath = '../../precompiles/solo/abi.json';
            const contractABI = require(contractABIPath);
            // Get a contract instance
            solo = new ethers.Contract(SoloPrecompileContract, contractABI, signer);
            soloAddr = await getKeyPaxAddress("solo");
            await fundPaxAddress(soloAddr);
            cw20Address = await deployWasm(WASM.CW20, soloAddr, "cw20", {
                name: "Test",
                symbol: "TEST",
                decimals: 6,
                initial_balances: [
                    { address: soloAddr, amount: "2000000" },
                ]
            });

        });

        it("Claims all CW20 balances from the old account", async function () {
            let signerAddr = await accounts[0].signer.getAddress();
            let claimMsg = await printClaimSpecificMsg("solo", signerAddr, "CW20", cw20Address);
            let payload = hex2uint8(claimMsg);
            const claim = await solo.claimSpecific(payload, {gasLimit: 200000});
            const receipt = await claim.wait();
            expect(receipt.status).to.equal(1);
            const result = await queryWasm(cw20Address, "balance", {address: soloAddr});
            expect(result.data.balance).to.equal('0');
            const claimerResult = await queryWasm(cw20Address, "balance", {address: accounts[0].paxAddress});
            expect(claimerResult.data.balance).to.equal('2000000');
        });
    });
});