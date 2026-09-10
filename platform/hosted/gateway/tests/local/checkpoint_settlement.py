#!/usr/bin/env python3
import hashlib
import http.client
import json
import os
from pathlib import Path
import runpy
import signal
import subprocess
import sys
import time


ROOT = Path(__file__).resolve().parents[5]
COMMON = runpy.run_path(str(ROOT / "tests/daemon/finality-authority-chain.py"))
Chain = COMMON["Chain"]
run = COMMON["run"]
ADMIN = COMMON["ADMIN"]
USDL = COMMON["USDL"]


def stop(_signal, _frame):
    raise SystemExit(0)


def main():
    work = Path(sys.argv[1]).resolve()
    binary = Path(sys.argv[2]).resolve()
    network_id = int(sys.argv[3])
    genesis = Path(sys.argv[4]).resolve()
    output = work / "checkpoint-output"
    output.mkdir(mode=0o700)
    artifacts = work / "checkpoint-contracts"
    cache = work / "checkpoint-contract-cache"
    run(
        "forge",
        "build",
        "contracts/GuarantorBond.sol",
        "contracts/CheckpointRegistry.sol",
        "platform/hosted/paxeer/contracts/BetaUsdl.sol",
        "--out",
        str(artifacts),
        "--cache-path",
        str(cache),
    )
    token = json.loads((artifacts / "BetaUsdl.sol/BetaUsdl.json").read_text())
    timestamp = int(time.time())
    chain_genesis = {
        "config": {"chainId": 31337},
        "timestamp": hex(timestamp),
        "gasLimit": "0x1c9c380",
        "difficulty": "0x0",
        "alloc": {
            USDL: {
                "balance": "0x0",
                "code": token["deployedBytecode"]["object"],
                "storage": {"0x" + "00" * 32: "0x" + "00" * 12 + ADMIN[2:]},
            },
            ADMIN: {"balance": hex(10**24)},
        },
    }
    genesis_path = work / "checkpoint-chain.json"
    genesis_path.write_text(json.dumps(chain_genesis))
    port = COMMON["free_port"]()
    chain = Chain(port)
    process = None
    try:
        with (work / "checkpoint-anvil.log").open("w") as log:
            process = subprocess.Popen(
                [
                    "anvil",
                    "--host",
                    "127.0.0.1",
                    "--port",
                    str(port),
                    "--chain-id",
                    "31337",
                    "--timestamp",
                    str(timestamp),
                    "--hardfork",
                    "cancun",
                    "--init",
                    str(genesis_path),
                    "--silent",
                ],
                cwd=ROOT,
                stdout=log,
                stderr=log,
            )
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            assert process.poll() is None, "checkpoint Anvil exited"
            try:
                assert chain.rpc("eth_chainId", []) == "0x7a69"
                break
            except (OSError, http.client.HTTPException):
                time.sleep(0.1)
        else:
            raise RuntimeError("checkpoint Anvil readiness deadline")
        asset = run("cast", "keccak", "USDL")
        word = COMMON["word"]
        bond = chain.deploy(
            json.loads(
                (artifacts / "GuarantorBond.sol/GuarantorBond.json").read_text()
            ),
            "constructor(address,address,address,address,bytes32,uint16,uint32,uint32,uint64,bytes32,uint192)",
            [
                ADMIN,
                ADMIN,
                USDL,
                USDL,
                asset,
                "3",
                str(network_id),
                "1000",
                "86400",
                word("a1"),
                str(1 << 128),
            ],
        )
        chain.send(USDL, "mint(address,uint256)", ADMIN, "2000")
        chain.send(USDL, "approve(address,uint256)", bond, "2000")
        for index, signer in enumerate(COMMON["SIGNERS"], 1):
            guarantor = "0x" + f"{index:064x}"
            chain.send(
                bond,
                "activateGuarantor(bytes32,address,address,uint64,uint64)",
                guarantor,
                signer,
                ADMIN,
                "1",
                str(index),
            )
            chain.send(bond, "depositBond(bytes32,uint256)", guarantor, "1000")
        registration = (genesis / "paxeer-registration-request.lxrr").read_bytes()
        assert len(registration) == 73
        state_root = "0x" + registration[9:41].hex()
        receipt_root = "0x" + registration[41:73].hex()
        manifest = "0x" + hashlib.sha256(
            (genesis / "genesis.manifest").read_bytes()
        ).hexdigest()
        registry = chain.deploy(
            json.loads(
                (artifacts / "CheckpointRegistry.sol/CheckpointRegistry.json").read_text()
            ),
            "constructor(address,uint16,uint32,uint16,uint16,uint64,uint64,bytes32,bytes32,bytes32,bytes32,uint192)",
            [
                bond,
                "3",
                str(network_id),
                "2",
                "32",
                "3600",
                "60",
                manifest,
                state_root,
                receipt_root,
                word("a2"),
                str(1 << 128),
            ],
        )
        (work / "checkpoint-chain-ready.json").write_text(
            json.dumps({"bond": bond, "registry": registry, "port": port})
        )
        header = output / "available-header.bin"
        deadline = time.monotonic() + 180
        while not header.is_file():
            if time.monotonic() >= deadline:
                raise RuntimeError("signed checkpoint header deadline")
            assert process.poll() is None
            time.sleep(0.05)
        environment = os.environ.copy()
        environment.update(
            LAYERX_NODE_PAXEER_CHAIN_ID="31337",
            LAYERX_NODE_SETTLEMENT_CONTRACT=bond,
            LAYERX_NODE_CHECKPOINT_REGISTRY=registry,
            LAYERX_NODE_PAXEER_RPC_ADDRESS="127.0.0.1",
            LAYERX_NODE_PAXEER_RPC_PORT=str(port),
            LAYERX_TEST_DA_HEADER_FILE=str(header),
        )
        vector = json.loads(run(str(binary), "prepare", env=environment))
        calldata = run(
            "cast",
            "calldata",
            f"registerCheckpoint({COMMON['HEADER']},bytes,{COMMON['ATTESTATION']}[])",
            vector["header"],
            "0x",
            vector["attestations"],
        )
        receipt = chain.transaction(calldata, registry)
        assert receipt["logs"], "checkpoint registration event missing"
        observed = (
            int(
                chain.rpc(
                    "eth_getBlockByNumber", [receipt["blockNumber"], False]
                )["timestamp"],
                16,
            )
            * 1000
        )
        run(
            str(binary),
            "emit",
            receipt["transactionHash"],
            str(int(receipt["blockNumber"], 16)),
            str(observed),
            str(output),
            env=environment,
        )
        (work / "checkpoint-finality-ready").write_text(vector["checkpoint_id"])
        while True:
            assert process.poll() is None
            time.sleep(0.5)
    finally:
        if process is not None and process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=10)


if __name__ == "__main__":
    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    main()
