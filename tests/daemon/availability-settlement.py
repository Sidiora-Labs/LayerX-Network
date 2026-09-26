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

ROOT = Path(__file__).resolve().parents[2]
COMMON = runpy.run_path(str(ROOT / 'tests/daemon/finality-authority-chain.py'))
Chain = COMMON['Chain']
run = COMMON['run']
ADMIN = COMMON['ADMIN']
USDL = COMMON['USDL']
ANCHOR = '0x0000000000000000000000000000000000001014'
ANCHOR_SOURCE = '''// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {CanonicalCheckpoint} from "../../../contracts/libraries/CanonicalCheckpoint.sol";
import {IGuarantorEligibility} from "../../../contracts/interfaces/IGuarantorEligibility.sol";

interface IAvailabilityCheckpointRegistry {
    function registerCheckpoint(
        CanonicalCheckpoint.HeaderCommitments calldata header,
        bytes calldata validityProof,
        CanonicalCheckpoint.GuarantorAttestation[] calldata attestations
    ) external returns (bytes32 digest);
}

contract AvailabilityAnchor is IGuarantorEligibility {
    struct Checkpoint {
        uint64 batchNumber;
        bytes32 checkpointId;
        bytes32 headerDigest;
        uint64 epoch;
        uint64 firstSequence;
        uint64 lastSequence;
        bytes32 previousStateRoot;
        bytes32 stateRoot;
        bytes32 receiptRoot;
        bytes32 dataAvailabilityRoot;
        bytes32 sequencerId;
        uint64 timestampMs;
        uint8 status;
        uint8 signers;
        uint8 availabilityMask;
        uint32 openChallenges;
        uint64 submittedHeight;
        uint64 finalizedHeight;
    }

    event CheckpointSubmitted(uint64 indexed batchNumber, bytes32 indexed checkpointId, bytes32 stateRoot, bytes32 receiptRoot, uint8 signers);
    event CheckpointFinalized(uint64 indexed batchNumber, bytes32 indexed checkpointId, bytes32 stateRoot, bytes32 receiptRoot);

    uint8 private constant STATUS_FINAL = 2;
    IGuarantorEligibility private immutable bond;
    IAvailabilityCheckpointRegistry public registry;
    mapping(uint64 => Checkpoint) private records;

    constructor(IGuarantorEligibility guarantorBond) {
        require(address(guarantorBond) != address(0));
        bond = guarantorBond;
    }

    function bindRegistry(IAvailabilityCheckpointRegistry checkpointRegistry) external {
        require(address(registry) == address(0) && address(checkpointRegistry) != address(0));
        registry = checkpointRegistry;
    }

    function protocolVersion() external view returns (uint16) {
        return bond.protocolVersion();
    }

    function networkId() external view returns (uint32) {
        return bond.networkId();
    }

    function slashingAuthority() external view returns (address) {
        return bond.slashingAuthority();
    }

    function membershipVersion() external view returns (uint64) {
        return bond.membershipVersion();
    }

    function bondedActive(bytes32 guarantorId, address signer, uint64 checkpointEpoch) external view returns (bool) {
        return bond.bondedActive(guarantorId, signer, checkpointEpoch);
    }

    function registerCheckpoint(
        CanonicalCheckpoint.HeaderCommitments calldata header,
        bytes calldata validityProof,
        CanonicalCheckpoint.GuarantorAttestation[] calldata attestations
    ) external returns (bytes32 checkpointId) {
        require(address(registry) != address(0) && records[header.batchNumber].status == 0);
        checkpointId = registry.registerCheckpoint(header, validityProof, attestations);
        Checkpoint storage record = records[header.batchNumber];
        record.batchNumber = header.batchNumber;
        record.checkpointId = checkpointId;
        record.headerDigest = sha256(CanonicalCheckpoint.encodeHeader(header));
        record.epoch = header.epoch;
        record.firstSequence = header.firstSequence;
        record.lastSequence = header.lastSequence;
        record.previousStateRoot = header.previousStateRoot;
        record.stateRoot = header.resultingStateRoot;
        record.receiptRoot = header.receiptMerkleRoot;
        record.dataAvailabilityRoot = header.dataAvailabilityRoot;
        record.sequencerId = header.sequencerId;
        record.timestampMs = header.timestamp;
        record.status = STATUS_FINAL;
        record.signers = uint8(attestations.length);
        record.availabilityMask = attestations[0].availabilityClassMask;
        record.submittedHeight = uint64(block.number);
        record.finalizedHeight = uint64(block.number);
        emit CheckpointSubmitted(header.batchNumber, checkpointId, header.resultingStateRoot, header.receiptMerkleRoot, uint8(attestations.length));
        emit CheckpointFinalized(header.batchNumber, checkpointId, header.resultingStateRoot, header.receiptMerkleRoot);
    }

    function statusOf(uint64 batchNumber) external view returns (uint8) {
        return records[batchNumber].status;
    }

    function checkpoint(uint64 batchNumber) external view returns (Checkpoint memory) {
        return records[batchNumber];
    }
}
'''


def stop(_signal, _frame):
    raise SystemExit(0)


def main():
    work = Path(sys.argv[1])
    binary = Path(sys.argv[2]).resolve()
    artifacts = ROOT / 'build/availability-contracts/artifacts'
    anchor_source = ROOT / 'build/availability-contracts/src/AvailabilityAnchor.sol'
    anchor_source.parent.mkdir(parents=True, exist_ok=True)
    anchor_source.write_text(ANCHOR_SOURCE)
    run('forge', 'build', 'contracts/GuarantorBond.sol', 'contracts/CheckpointRegistry.sol',
        'platform/hosted/paxeer/contracts/BetaUsdl.sol', str(anchor_source), '--out', str(artifacts),
        '--cache-path', str(ROOT / 'build/availability-contracts/cache'))
    token = json.loads((artifacts / 'BetaUsdl.sol/BetaUsdl.json').read_text())
    timestamp = int(time.time())
    genesis = {'config': {'chainId': 31337}, 'timestamp': hex(timestamp),
               'gasLimit': '0x1c9c380', 'difficulty': '0x0', 'alloc': {
                   USDL: {'balance': '0x0', 'code': token['deployedBytecode']['object'],
                          'storage': {'0x' + '00' * 32: '0x' + '00' * 12 + ADMIN[2:]}},
                   ADMIN: {'balance': hex(10 ** 24)}}}
    genesis_path = work / 'availability-chain.json'
    genesis_path.write_text(json.dumps(genesis))
    port = COMMON['free_port']()
    chain = Chain(port)
    process = None
    try:
        with (work / 'availability-anvil.log').open('w') as log:
            process = subprocess.Popen(['anvil', '--host', '127.0.0.1', '--port', str(port),
                '--chain-id', '31337', '--timestamp', str(timestamp), '--hardfork', 'cancun',
                '--init', str(genesis_path), '--silent'], cwd=ROOT, stdout=log, stderr=log)
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            assert process.poll() is None, 'availability Anvil exited'
            try:
                assert chain.rpc('eth_chainId', []) == '0x7a69'
                break
            except (OSError, http.client.HTTPException):
                time.sleep(.1)
        else:
            raise RuntimeError('availability Anvil readiness deadline')
        asset = run('cast', 'keccak', 'USDL')
        word = COMMON['word']
        bond = chain.deploy(json.loads((artifacts / 'GuarantorBond.sol/GuarantorBond.json').read_text()),
            'constructor(address,address,address,address,bytes32,uint16,uint32,uint32,uint64,bytes32,uint192)',
            [ADMIN, ADMIN, USDL, USDL, asset, '3', '77', '1000', '86400', word('a1'), str(1 << 128)])
        chain.send(USDL, 'mint(address,uint256)', ADMIN, '2000')
        chain.send(USDL, 'approve(address,uint256)', bond, '2000')
        for index, signer in enumerate(COMMON['SIGNERS'], 1):
            guarantor = '0x' + f'{index:064x}'
            chain.send(bond, 'activateGuarantor(bytes32,address,address,uint64,uint64)',
                       guarantor, signer, ADMIN, '1', str(index))
            chain.send(bond, 'depositBond(bytes32,uint256)', guarantor, '1000')
        registration = (work / 'data/genesis/paxeer-registration-request.lxrr').read_bytes()
        assert len(registration) == 73
        state_root = '0x' + registration[9:41].hex()
        receipt_root = '0x' + registration[41:73].hex()
        manifest = '0x' + hashlib.sha256((work / 'data/genesis/genesis.manifest').read_bytes()).hexdigest()
        anchor = chain.deploy(json.loads((artifacts / 'AvailabilityAnchor.sol/AvailabilityAnchor.json').read_text()),
            'constructor(address)', [bond])
        anchor_code = chain.rpc('eth_getCode', [anchor, 'latest'])
        chain.rpc('anvil_setCode', [ANCHOR, anchor_code])
        assert chain.rpc('eth_getCode', [ANCHOR, 'latest']) == anchor_code
        registry = chain.deploy(json.loads((artifacts / 'CheckpointRegistry.sol/CheckpointRegistry.json').read_text()),
            'constructor(address,uint16,uint32,uint16,uint16,uint64,uint64,bytes32,bytes32,bytes32,bytes32,uint192)',
            [ANCHOR, '3', '77', '2', '32', '3600', '60', manifest, state_root, receipt_root,
             word('a2'), str(1 << 128)])
        chain.send(ANCHOR, 'bindRegistry(address)', registry)
        ready = work / 'availability-chain-ready.json'
        ready.write_text(json.dumps({'anchor': ANCHOR, 'bond': bond, 'registry': registry, 'port': port}))
        header = work / 'availability-output/available-header.bin'
        deadline = time.monotonic() + 180
        while not header.is_file():
            if time.monotonic() >= deadline:
                raise RuntimeError('signed availability header deadline')
            assert process.poll() is None
            time.sleep(.1)
        env = os.environ.copy()
        env.update(LAYERX_NODE_PAXEER_CHAIN_ID='31337', LAYERX_NODE_SETTLEMENT_CONTRACT=ANCHOR,
                   LAYERX_NODE_CHECKPOINT_REGISTRY=ANCHOR,
                   LAYERX_NODE_PAXEER_RPC_ADDRESS='127.0.0.1', LAYERX_NODE_PAXEER_RPC_PORT=str(port),
                   LAYERX_TEST_DA_HEADER_FILE=str(header))
        vector = json.loads(run(str(binary), 'prepare', str(work / 'availability-output'), env=env))
        calldata = run('cast', 'calldata',
                       f"registerCheckpoint({COMMON['HEADER']},bytes,{COMMON['ATTESTATION']}[])",
                       vector['header'], '0x', vector['attestations'])
        receipt = chain.transaction(calldata, ANCHOR)
        assert receipt['logs'], 'checkpoint registration event missing'
        observed = int(chain.rpc('eth_getBlockByNumber', [receipt['blockNumber'], False])['timestamp'], 16) * 1000
        run(str(binary), 'emit', receipt['transactionHash'],
            str(int(receipt['blockNumber'], 16)), str(observed), str(work / 'availability-output'), env=env)
        (work / 'availability-finality-ready').write_text('registered\n')
        while True:
            assert process.poll() is None
            time.sleep(.5)
    finally:
        if process is not None and process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=10)


if __name__ == '__main__':
    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    main()
