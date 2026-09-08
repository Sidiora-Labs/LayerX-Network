#!/usr/bin/env python3
import copy
import importlib.util
import json
import os
import struct
import tempfile
from pathlib import Path
import unittest
from eth_keys.exceptions import BadSignature

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('settlement', ROOT / 'cmd/layerx-guarantor/settlement.py')
s = importlib.util.module_from_spec(spec)
spec.loader.exec_module(s)


class SettlementTests(unittest.TestCase):
    def setUp(self):
        self.vector = json.loads((ROOT / 'tests/vectors/checkpoint/fresh.json').read_text())
        self.header = s.values(s.HEADER_TYPES, list(self.vector['header'].values())[:15])
        self.digest = s.raw(self.vector['expected_digest'], 32)
        self.bond = '0x2e234dae75c793f67a35089c9d99245e1c58470b'
        self.attestations = []
        for a in self.vector['attestations']:
            signature = s.raw(a['signature'], 64)
            self.attestations.append((2, 42, 31337, self.bond, self.header[2], self.digest, self.digest, s.raw(a['guarantor_id']), self.header[3], self.header[11], True, True, 31, a['attested_at_ms'], a['signer'], signature[:32], signature[32:], a['signature_v']))

    def test_canonical_hash_and_abi_roundtrip(self):
        self.assertEqual(s.checkpoint_hash(self.header, s.raw(self.vector['certificate']['validity_proof'])), self.digest)
        encoded = s.calldata('registerCheckpoint(' + s.HEADER + ',bytes,' + s.ATTESTATION + '[])', (s.HEADER, 'bytes', s.ATTESTATION + '[]'), (self.header, b'PROOF', self.attestations))
        decoded = s.decode((s.HEADER, 'bytes', s.ATTESTATION + '[]'), s.raw(encoded)[4:])
        self.assertEqual(decoded, (self.header, b'PROOF', tuple(self.attestations)))

    def test_real_vector_signatures(self):
        for a in self.attestations:
            s.validate_attestation(a, self.header, self.digest, 31337, self.bond, 3_600_000)

    def test_signature_domain_root_and_stale_refusals(self):
        a = self.attestations[0]
        for index, replacement in [(2, 125), (5, bytes(32)), (9, bytes(32)), (10, False), (12, 0), (13, 999_999), (13, 4_600_001), (14, self.attestations[1][14]), (17, 29)]:
            with self.subTest(field=index, replacement=replacement):
                altered = list(a)
                altered[index] = replacement
                with self.assertRaises((ValueError, BadSignature)):
                    s.validate_attestation(tuple(altered), self.header, self.digest, 31337, self.bond, 3_600_000)

    def test_exact_receipt_event_and_negative_fields(self):
        registry = '0x' + '12' * 20
        tx = '0x' + '34' * 32
        block_hash = '0x' + '56' * 32
        data = s.encode(['uint64', 'uint64', 'bytes32', 'bytes32', 'bytes32', 'uint64'], [self.header[4], self.header[5], self.header[6], self.header[7], self.header[11], 4])
        log = {'address': registry, 'transactionHash': tx, 'blockHash': block_hash, 'blockNumber': '0x1', 'removed': False, 'topics': [s.EVENT, '0x' + self.digest.hex(), '0x' + s.encode(['uint64'], [1]).hex(), '0x' + s.encode(['uint64'], [1]).hex()], 'data': '0x' + data.hex()}
        receipt = {'status': '0x1', 'transactionHash': tx, 'to': registry, 'blockHash': block_hash, 'blockNumber': '0x1', 'logs': [log]}
        self.assertEqual(s.validate_receipt(receipt, registry, tx, self.digest, self.header, 4), 1)
        for key, value in [('removed', True), ('data', '0x' + bytes(192).hex()), ('transactionHash', '0x' + 'ff' * 32), ('blockHash', '0x' + 'ff' * 32), ('blockNumber', '0x2')]:
            changed = copy.deepcopy(receipt)
            changed['logs'][0][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                s.validate_receipt(changed, registry, tx, self.digest, self.header, 4)
        for logs in [[], [log, log]]:
            with self.assertRaises(ValueError):
                s.validate_receipt(dict(receipt, logs=logs), registry, tx, self.digest, self.header, 4)
        with self.assertRaises(ValueError):
            s.validate_receipt(dict(receipt, status='0x0'), registry, tx, self.digest, self.header, 4)

    def test_real_submitter_transaction_signing(self):
        account = s.Account.create()
        transaction = {'chainId': 31337, 'nonce': 0, 'to': s.to_checksum_address(self.bond), 'value': 0, 'gas': 100000, 'gasPrice': 1000000000, 'data': s.calldata('membershipVersion()')}
        signed = account.sign_transaction(transaction)
        self.assertEqual(s.Account.recover_transaction(signed.raw_transaction), account.address)
        self.assertEqual(s.keccak(bytes(signed.raw_transaction)), signed.hash)

    def test_wire_membership_and_registration(self):
        member = {'guarantor_id': '0x' + self.attestations[0][7].hex(), 'signer': self.attestations[0][14], 'bonded_active': True, 'bond_amount': 1000, 'joined_epoch': 1, 'authorization_version': 2}
        wire = s.wire_encode('membership', {'version': 4, 'threshold': 2, 'maximum_attestation_delay_ms': 3600000, 'minimum_bond': 100, 'members': [member]})
        self.assertEqual(len(wire), 125)
        self.assertEqual(struct.unpack('>QIQI', wire[:24]), (4, 2, 3600000, 1))
        self.assertEqual(int.from_bytes(wire[24:40], 'big'), 100)
        self.assertEqual(wire[40:72], self.attestations[0][7])
        self.assertEqual(int.from_bytes(wire[93:109], 'big'), 1000)
        registration = s.wire_encode('register', {'already_registered': True, 'transaction_id': '0x' + '12' * 32, 'observed_block_number': 5, 'observed_at_ms': 1000000, 'set_version': 4})
        self.assertEqual(len(registration), 57)
        self.assertEqual(struct.unpack('>QQQ', registration[33:]), (5, 1000000, 4))

    def test_configuration_real_vector_public_keys_and_environment(self):
        document = json.loads((ROOT / 'contracts/config/checkpoint-settlement.json').read_text())
        domain = copy.deepcopy(document['settlement_domains']['vectors'])
        domain['guarantor_bond'] = domain['settlement_contract']
        domain['settlement_contract'] = '0x' + '23' * 20
        document['settlement_domains']['beta'] = domain
        environment = {'LAYERX_NODE_PAXEER_CHAIN_ID': str(domain['paxeer_chain_id']), 'LAYERX_NODE_SETTLEMENT_CONTRACT': domain['guarantor_bond'], 'LAYERX_NODE_CHECKPOINT_REGISTRY': domain['settlement_contract']}
        previous = {key: os.environ.get(key) for key in environment}
        try:
            os.environ.update(environment)
            with tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / 'settlement.json'
                path.write_text(json.dumps(document))
                request = {'settlement_file': str(path), 'settlement_domain': 'beta'}
                configured = s.configuration(request)
                wire = s.wire_encode('config', configured)
                self.assertEqual(len(wire), 56 + 65 * 3)
                self.assertEqual(struct.unpack('>QI', wire[:12]), (31337, 42))
                os.environ['LAYERX_NODE_PAXEER_CHAIN_ID'] = '125'
                with self.assertRaises(ValueError):
                    s.configuration(request)
        finally:
            for key, value in previous.items():
                if value is None:
                    os.environ.pop(key, None)
                else:
                    os.environ[key] = value

    def test_rpc_plaintext_restricted_to_local_relay(self):
        with self.assertRaises(ValueError):
            s.RPC('http://example.com')
        with self.assertRaises(ValueError):
            s.RPC('http://user:secret@127.0.0.1')
        s.RPC('http://127.0.0.1:12345')


if __name__ == '__main__':
    unittest.main()
