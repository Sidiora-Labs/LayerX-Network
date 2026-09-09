#!/usr/bin/env python3
import hashlib
import http.client
import json
import os
from pathlib import Path
import runpy
import signal
import socket
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
COMMON = runpy.run_path(str(ROOT / "tests/daemon/finality-authority-chain.py"))
Chain, run, ADMIN, USDL = (COMMON[key] for key in ("Chain", "run", "ADMIN", "USDL"))


def read_env(path):
    return dict(line.split("=", 1) for line in path.read_text().splitlines() if line)


def client_identity():
    os.setgroups([ROOT.stat().st_gid])
    os.setgid(4021)
    os.setuid(4021)


def await_condition(condition, processes, message, seconds=30):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        for process in processes:
            if process.poll() is not None:
                raise RuntimeError(f"child process exited while waiting for {message}: {process.returncode}")
        try:
            if condition():
                return
        except (OSError, http.client.HTTPException):
            pass
        time.sleep(0.1)
    raise RuntimeError(f"deadline waiting for {message}")


def main():
    if os.geteuid() != 0:
        raise RuntimeError("real LNI integration needs root to run the client as distinct uid4021")
    native = Path(os.environ.get("LAYERX_TEST_NATIVE_BIN_DIR", ROOT / "build/bin"))
    build = Path(os.environ.get("LAYERX_TEST_BUILD_DIR", ROOT / "build"))
    probe = Path(sys.argv[1]).resolve() if len(sys.argv) == 2 else build / "tests/lxp_test_guarantor_integration"
    admission = build / "tests/lxp_test_program_admission"
    for binary in (native / "layerxd", native / "layerx-genesis-build", probe, admission):
        if not binary.is_file():
            raise RuntimeError(f"required real binary not built: {binary}")
    logs = ROOT / "qual-logs/gp1"
    logs.mkdir(parents=True, exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix="guarantor-daemon-", dir=logs))
    work.chmod(0o755)
    print(f"guarantor-integration: evidence {work}", flush=True)
    processes = []
    handles = []

    def launch(command, name, env=None, **kwargs):
        handle = (work / name).open("w")
        handles.append(handle)
        process = subprocess.Popen(command, cwd=ROOT, stdout=handle, stderr=handle, env=env, **kwargs)
        processes.append(process)
        return process

    try:
        artifacts = build / "guarantor-contracts/artifacts"
        run("forge", "build", "contracts/GuarantorBond.sol", "contracts/CheckpointRegistry.sol",
            "platform/hosted/paxeer/contracts/BetaUsdl.sol", "--out", str(artifacts),
            "--cache-path", str(build / "guarantor-contracts/cache"))
        token = json.loads((artifacts / "BetaUsdl.sol/BetaUsdl.json").read_text())
        timestamp = int(time.time())
        genesis = {"config": {"chainId": 31337}, "timestamp": hex(timestamp), "gasLimit": "0x1c9c380", "difficulty": "0x0", "alloc": {
            USDL: {"balance": "0x0", "code": token["deployedBytecode"]["object"], "storage": {"0x" + "00" * 32: "0x" + "00" * 12 + ADMIN[2:]}},
            ADMIN: {"balance": hex(10 ** 24)}}}
        (work / "anvil-genesis.json").write_text(json.dumps(genesis))
        rpc_port = COMMON["free_port"]()
        chain = Chain(rpc_port)
        anvil = launch(["anvil", "--host", "127.0.0.1", "--port", str(rpc_port), "--chain-id", "31337", "--timestamp", str(timestamp), "--hardfork", "cancun", "--init", str(work / "anvil-genesis.json"), "--silent"], "anvil.log")
        await_condition(lambda: chain.rpc("eth_chainId", []) == "0x7a69", [anvil], "Anvil chain31337")
        for name, seed in [("sequencer", 0x22), ("treasury", 0x11)]:
            (work / name).write_bytes(bytes([seed]) * 32)
            (work / name).chmod(0o600)
        program_port, replica_port = COMMON["free_port"](), COMMON["free_port"]()
        with (work / "bootstrap.log").open("w") as log:
            subprocess.run(["bash", str(ROOT / "platform/hosted/node/bootstrap.sh"), "--data-dir", str(work / "data"), "--run-dir", str(work / "run"), "--network-id", "77", "--sequencer-key", str(work / "sequencer"), "--treasury-key", str(work / "treasury"), "--lni-uid", "4021", "--lni-gid", "4021", "--program-port", str(program_port), "--replica-port", str(replica_port), "--layerxd", str(native / "layerxd"), "--genesis-build", str(native / "layerx-genesis-build"), "--settlement-env", str(work / "settlement.env")], cwd=ROOT, stdout=log, stderr=log, check=True)
        node = read_env(work / "data/node.env")
        asset = run("cast", "keccak", "USDL")
        bond = chain.deploy(json.loads((artifacts / "GuarantorBond.sol/GuarantorBond.json").read_text()), "constructor(address,address,address,address,bytes32,uint16,uint32,uint32,uint64,bytes32,uint192)", [ADMIN, ADMIN, USDL, USDL, asset, "3", "77", "1000", "86400", COMMON["word"]("a1"), str(1 << 128)])
        chain.send(USDL, "mint(address,uint256)", ADMIN, "2000")
        chain.send(USDL, "approve(address,uint256)", bond, "2000")
        guarantors = []
        domain_helper = runpy.run_path(str(ROOT / "platform/hosted/paxeer/settlement-domain.py"))
        for index, prefix in enumerate(("LAYERX_NODE_GENESIS_GUARANTOR", "LAYERX_NODE_SECOND_GUARANTOR"), 1):
            public_key = node[prefix + "_PUBLIC_KEY"]
            signer = "0x" + domain_helper["signer_of"](bytes.fromhex(public_key)).hex()
            identifier = "0x" + node[prefix + "_ID"]
            chain.send(bond, "activateGuarantor(bytes32,address,address,uint64,uint64)", identifier, signer, ADMIN, "1", str(index))
            chain.send(bond, "depositBond(bytes32,uint256)", identifier, "1000")
            guarantors.append({"guarantor_id": identifier, "public_key": "0x" + public_key, "signer": signer})
        request = (work / "data/genesis/paxeer-registration-request.lxrr").read_bytes()
        if len(request) != 73:
            raise RuntimeError("invalid real genesis registration request")
        state_root, receipt_root = "0x" + request[9:41].hex(), "0x" + request[41:73].hex()
        manifest = "0x" + hashlib.sha256((work / "data/genesis/genesis.manifest").read_bytes()).hexdigest()
        registry = chain.deploy(json.loads((artifacts / "CheckpointRegistry.sol/CheckpointRegistry.json").read_text()), "constructor(address,uint16,uint32,uint16,uint16,uint64,uint64,bytes32,bytes32,bytes32,bytes32,uint192)", [bond, "3", "77", "2", "32", "3600", "60", manifest, state_root, receipt_root, COMMON["word"]("a2"), str(1 << 128)])
        settlement = {"LAYERX_NODE_PAXEER_CHAIN_ID": "31337", "LAYERX_NODE_SETTLEMENT_CONTRACT": bond, "LAYERX_NODE_CHECKPOINT_REGISTRY": registry, "LAYERX_NODE_PAXEER_RPC_ADDRESS": "127.0.0.1", "LAYERX_NODE_PAXEER_RPC_PORT": str(rpc_port)}
        (work / "settlement.env").write_text("".join(f"{key}={value}\n" for key, value in settlement.items()))
        domain = json.loads((ROOT / "contracts/config/checkpoint-settlement.json").read_text())
        domain["settlement_domains"]["beta"] = {"protocol_version": 3, "paxeer_chain_id": 31337, "network_id": 77, "settlement_contract": registry, "guarantor_bond": bond, "guarantor_set": sorted(guarantors, key=lambda g: g["guarantor_id"])}
        (work / "checkpoint-settlement.json").write_text(json.dumps(domain))
        replica_env = os.environ | read_env(work / "data/replica.env")
        replica = launch([str(native / "layerxd"), "--authority-replica", str(work / "data/replica.conf")], "replica.log", env=replica_env)
        def replica_ready():
            with socket.create_connection(("127.0.0.1", replica_port), timeout=1):
                return True
        await_condition(replica_ready, [anvil, replica], "real receipt authority")
        daemon_env = os.environ | read_env(work / "data/sequencer.env") | settlement
        daemon = launch([str(native / "layerxd"), "--serve", str(work / "data/sequencer.conf")], "daemon.log", env=daemon_env)
        lni_socket = str(work / "run/layerxd.lni.sock")
        await_condition(lambda: Path(lni_socket).is_socket(), [anvil, replica, daemon], "real sequencer LNI")
        with (work / "admission.log").open("w") as log:
            subprocess.run([str(admission), lni_socket, "--availability-batches"], cwd=ROOT, preexec_fn=client_identity, stdout=log, stderr=log, check=True, timeout=60)
        runtime_state = work / "runtime-replay"
        runtime_state.mkdir(mode=0o700)
        with (work / "runtime.log").open("w") as log:
            runtime_result = subprocess.run([str(build / "tests/lxp_test_guarantor_runtime"), str(work / "data/sequencer.conf"), str(runtime_state), str(work / "data/checkpoints/da-bodies.log"), "9"], cwd=ROOT, env=daemon_env, stdout=log, stderr=log, timeout=120)
        print((work / "runtime.log").read_text(), end="", flush=True)
        print(f"guarantor-integration: independent runtime qualification exit={runtime_result.returncode}", flush=True)
        output = work / "candidate"
        output.mkdir()
        output.chmod(0o750)
        os.chown(output, 4021, 4021)
        with (work / "candidate.log").open("w") as log:
            result = subprocess.run([str(probe), lni_socket, "1", "77", node["LAYERX_NODE_SEQUENCER_ID"], node["LAYERX_NODE_SEQUENCER_PUBLIC_KEY"], str(output / "batch-1.body")], cwd=ROOT, preexec_fn=client_identity, stdout=log, stderr=log, timeout=30)
        print((work / "candidate.log").read_text(), end="", flush=True)
        if result.returncode or runtime_result.returncode:
            raise RuntimeError(f"runtime qualification exit={runtime_result.returncode}; candidate fetch exit={result.returncode}; both must pass before attestation or registration")
        print("guarantor-integration: real signed candidate range is available and verified", flush=True)
        from eth_account import Account
        submitter = Account.create()
        submitter_file = work / "submitter.key"
        submitter_file.write_text("0x" + submitter.key.hex())
        submitter_file.chmod(0o600)
        os.chown(submitter_file, 4021, 4021)
        funding = chain.rpc("eth_sendTransaction", [{"from": ADMIN, "to": submitter.address, "value": hex(10 ** 20)}])
        await_condition(lambda: chain.rpc("eth_getTransactionReceipt", [funding]), [anvil], "checkpoint submitter funding")
        tls_script = """
set -euo pipefail
. "$1/platform/hosted/tests/beta-cluster.sh"
WORK_DIR=$2
CA_DIR="$WORK_DIR/ca"
SECRETS_DIR="$WORK_DIR/tls-secrets"
ca_generate
chgrp -R 4021 "$CA_DIR"
find "$CA_DIR" -type d -exec chmod 0750 {} +
for identity in 1 2; do chmod 0440 "$CA_DIR/guarantor-$identity/key.pem"; done
"""
        with (work / "tls.log").open("w") as log:
            subprocess.run(["bash", "-c", tls_script, "guarantor-tls", str(ROOT), str(work)], check=True, stdout=log, stderr=log)
        shared = work / "submitter-lock"
        shared.mkdir(mode=0o770)
        os.chown(shared, 4021, 4021)
        ports = [COMMON["free_port"](), COMMON["free_port"]()]
        producer_processes = []
        for index, prefix in enumerate(("LAYERX_NODE_GENESIS_GUARANTOR", "LAYERX_NODE_SECOND_GUARANTOR")):
            identity = work / f"guarantor-{index + 1}/identity"
            state = work / f"producer-{index + 1}"
            state.mkdir(mode=0o700)
            os.chown(state, 4021, 4021)
            producer_env = os.environ | settlement | {
                "LAYERX_NODE_FIRST_BATCH": "1", "LAYERX_NODE_LAST_BATCH": "18446744073709551615",
                "LAYERX_NODE_NETWORK_ID": "77", "LAYERX_NODE_ASSET_ID": node["LAYERX_NODE_ASSET_ID"],
                "LAYERX_NODE_SEQUENCER_ID": node["LAYERX_NODE_SEQUENCER_ID"],
                "LAYERX_NODE_SEQUENCER_PUBLIC_KEY": node["LAYERX_NODE_SEQUENCER_PUBLIC_KEY"],
                "LAYERX_NODE_SNAPSHOT": str(identity / "genesis.lxs"),
                "LAYERX_NODE_GENESIS_MANIFEST": str(identity / "genesis.manifest"),
                "LAYERX_NODE_GENESIS_REGISTRATION": str(identity / "genesis.registration"),
                "LAYERX_NODE_IDENTITIES": str(identity / "identities.txt"),
                "LAYERX_GUARANTOR_NODE_CONFIG": str(identity / "node.conf"),
                "LAYERX_GUARANTOR_ID": node[prefix + "_ID"],
                "LAYERX_GUARANTOR_KEY_FILE": str(identity / "key.pem"),
                "LAYERX_GUARANTOR_STATE_DIR": str(state),
                "LAYERX_GUARANTOR_LNI_SOCKET": lni_socket,
                "LAYERX_GUARANTOR_SETTLEMENT_FILE": str(work / "checkpoint-settlement.json"),
                "LAYERX_GUARANTOR_SETTLEMENT_DOMAIN": "beta",
                "LAYERX_GUARANTOR_SUBMITTER_KEY_FILE": str(submitter_file),
                "LAYERX_GUARANTOR_SUBMITTER_LOCK_FILE": str(shared / "submitter.lock"),
                "LAYERX_GUARANTOR_PYTHON": sys.executable,
                "LAYERX_GUARANTOR_SETTLEMENT_HELPER": str(ROOT / "cmd/layerx-guarantor/settlement.py"),
                "LAYERX_GUARANTOR_LISTEN_PORT": str(ports[index]),
                "LAYERX_GUARANTOR_PEER_URL": f"https://127.0.0.1:{ports[1-index]}",
                "LAYERX_GUARANTOR_TLS_CA_FILE": str(work / "ca/ca.crt"),
                "LAYERX_GUARANTOR_TLS_CERT_FILE": str(work / f"ca/guarantor-{index + 1}/cert.pem"),
                "LAYERX_GUARANTOR_TLS_KEY_FILE": str(work / f"ca/guarantor-{index + 1}/key.pem"),
            }
            producer_processes.append(launch([str(native / "layerx-guarantor"), "--once"], f"producer-{index + 1}.log", env=producer_env, preexec_fn=client_identity))
        for process in producer_processes:
            if process.wait(timeout=120) != 0:
                raise RuntimeError("guarantor --once refused; inspect producer logs")
        producer_logs = [(work / f"producer-{index}.log").read_text() for index in (1, 2)]
        if not all("attested batch=1" in log for log in producer_logs):
            raise RuntimeError("both real guarantors must attest")
        if sum("registered batch=1" in log for log in producer_logs) != 1:
            raise RuntimeError("exactly one guarantor must register")
        if sum("observed registration batch=1" in log for log in producer_logs) != 1:
            raise RuntimeError("second guarantor must observe existing registration")
        if int(chain.rpc("eth_getTransactionCount", [submitter.address, "latest"]), 16) != 1:
            raise RuntimeError("dedicated submitter must send exactly one registration transaction")
        for index in (1, 2):
            state = work / f"producer-{index}"
            if len(list(state.glob("00000000000000000001-*.attestation"))) != 2:
                raise RuntimeError("two canonical attestations must be durably present per guarantor")
            for suffix in ("checkpoint", "finality"):
                if not (state / f"00000000000000000001.{suffix}").is_file():
                    raise RuntimeError(f"missing durable {suffix} evidence")
        print("guarantor-integration: two mTLS guarantors attested; one registered and one observed registration; tag28 feedback accepted", flush=True)
        verifier_state = work / "verifier"
        verifier_state.mkdir(mode=0o700)
        os.chown(verifier_state, 4021, 4021)
        verifier_env = producer_env | {"LAYERX_GUARANTOR_STATE_DIR": str(verifier_state)}
        state = work / "producer-1"
        attestations = sorted(state.glob("00000000000000000001-*.attestation"))
        with (work / "verify.log").open("w") as log:
            subprocess.run([str(probe), lni_socket, "1", "77", node["LAYERX_NODE_SEQUENCER_ID"], node["LAYERX_NODE_SEQUENCER_PUBLIC_KEY"], str(output / "verified-batch-1.body"), *(str(path) for path in attestations), str(state / "00000000000000000001.checkpoint"), str(state / "00000000000000000001.finality")], cwd=ROOT, env=verifier_env, preexec_fn=client_identity, stdout=log, stderr=log, check=True, timeout=60)
        print((work / "verify.log").read_text(), end="", flush=True)


    finally:
        for process in reversed(processes):
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=10)
        for handle in handles:
            handle.close()


def terminate(signum, _frame):
    raise SystemExit(128 + signum)


if __name__ == "__main__":
    signal.signal(signal.SIGTERM, terminate)
    signal.signal(signal.SIGINT, terminate)
    try:
        main()
    except subprocess.CalledProcessError as error:
        if error.stdout:
            sys.stderr.write(error.stdout)
        if error.stderr:
            sys.stderr.write(error.stderr)
        raise
