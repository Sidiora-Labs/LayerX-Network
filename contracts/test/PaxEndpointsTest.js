const { expect } = require("chai");
const { ethers } = require('hardhat');
const { deployWasm, ABI, WASM, executeWasm, deployErc20PointerForCw20, getAdmin, setupSigners } = require("./lib")

describe("Pax Endpoints Tester", function () {
    let accounts;
    let admin;
    let cw20Address;
    let pointer;

    before(async function () {
        accounts = await setupSigners(await hre.ethers.getSigners());
        admin = await getAdmin();

        cw20Address = await deployWasm(WASM.CW20, accounts[0].paxAddress, "cw20", {
            name: "Test",
            symbol: "TEST",
            decimals: 6,
            initial_balances: [
                { address: admin.paxAddress, amount: "1000000" },
                { address: accounts[0].paxAddress, amount: "2000000" },
                { address: accounts[1].paxAddress, amount: "3000000" }
            ],
            mint: {
                "minter": admin.paxAddress, "cap": "99900000000"
            }
        });
        // deploy a pointer
        const pointerAddr = await deployErc20PointerForCw20(hre.ethers.provider, cw20Address);
        const contract = new hre.ethers.Contract(pointerAddr, ABI.ERC20, hre.ethers.provider);
        pointer = contract.connect(accounts[0].signer);
    });

    it("Should emit a synthetic event upon transfer", async function () {
        const res = await executeWasm(cw20Address,  { transfer: { recipient: accounts[1].paxAddress, amount: "1" } });
        const blockNumber = parseInt(res["height"], 10);
        // look for synthetic event on evm pax_ endpoints
        const filter = {
            fromBlock: '0x' + blockNumber.toString(16),
            toBlock: '0x' + blockNumber.toString(16),
            address: pointer.address,
            topics: [ethers.id("Transfer(address,address,uint256)")]
        };
        const paxlogs = await ethers.provider.send('pax_getLogs', [filter]);
        expect(paxlogs.length).to.equal(1);
    });

    it("pax_getBlockByNumberExcludeTraceFail should not have the synthetic tx", async function () {
        // create a synthetic tx
        const res = await executeWasm(cw20Address,  { transfer: { recipient: accounts[1].paxAddress, amount: "1" } });
        const blockNumber = parseInt(res["height"], 10);

        // Query pax_getBlockByNumber - should have synthetic tx
        const paxBlock = await ethers.provider.send('pax_getBlockByNumber', ['0x' + blockNumber.toString(16), false]);
        expect(paxBlock.transactions.length).to.equal(1);

        // Query pax_getBlockByNumberExcludeTraceFail - should not have synthetic tx
        const paxBlockExcludeTrace = await ethers.provider.send('pax_getBlockByNumberExcludeTraceFail', ['0x' + blockNumber.toString(16), false]);
        expect(paxBlockExcludeTrace.transactions.length).to.equal(0);
    });

    it("pax_traceBlockByNumberExcludeTraceFail should not have synthetic tx", async function () {
        // create a synthetic tx
        const res = await executeWasm(cw20Address,  { transfer: { recipient: accounts[1].paxAddress, amount: "1" } });
        const blockNumber = parseInt(res["height"], 10);
        const paxBlockExcludeTrace = await ethers.provider.send('pax_traceBlockByNumberExcludeTraceFail', ['0x' + blockNumber.toString(16), {"tracer": "callTracer"}]);
        expect(paxBlockExcludeTrace.length).to.equal(0);
    });

    it("pax_traceBlockByHashExcludeTraceFail should not have synthetic tx", async function () {
        // create a synthetic tx
        const res = await executeWasm(cw20Address,  { transfer: { recipient: accounts[1].paxAddress, amount: "1" } });
        const blockNumber = parseInt(res["height"], 10);
        // get the block hash
        const block = await ethers.provider.send('eth_getBlockByNumber', ['0x' + blockNumber.toString(16), false]);
        const blockHash = block.hash;
        // check pax_getBlockByHash
        const paxBlock = await ethers.provider.send('pax_getBlockByHash', [blockHash, false]);
        expect(paxBlock.transactions.length).to.equal(1);
        // check pax_traceBlockByHashExcludeTraceFail
        const paxBlockExcludeTrace = await ethers.provider.send('pax_traceBlockByHashExcludeTraceFail', [blockHash, {"tracer": "callTracer"}]);
        expect(paxBlockExcludeTrace.length).to.equal(0);
    });
})
