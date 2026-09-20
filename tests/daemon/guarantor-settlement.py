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

    def anchor_vector(self):
        document = json.loads((ROOT / 'layerxproof/testdata/anchor_vectors.json').read_text())
        case = next(v for v in document['checkpoints'] if v['name'] == 'batch_1_quorum' and v['valid'])
        context = document['context'][0]
        encoded = bytes.fromhex(case['header'])
        self.assertEqual((len(encoded), encoded[:5]), (354, bytes.fromhex('000217010f')))
        header, cursor = [], 5
        for index, kind in enumerate(s.HEADER_TYPES, 1):
            self.assertEqual(encoded[cursor], index)
            cursor += 1
            if kind == 'bytes32':
                self.assertEqual(encoded[cursor:cursor + 4], (32).to_bytes(4, 'big'))
                header.append(encoded[cursor + 4:cursor + 36])
                cursor += 36
            else:
                width = int(kind[4:]) // 8
                header.append(int.from_bytes(encoded[cursor:cursor + width], 'big'))
                cursor += width
        self.assertEqual(cursor, 354)
        certificate = bytes.fromhex(case['certificate'])
        self.assertEqual(certificate[:6] + certificate[6:360], (1).to_bytes(2, 'big') + (354).to_bytes(4, 'big') + encoded)
        proof_length = int.from_bytes(certificate[360:364], 'big')
        proof = certificate[364:364 + proof_length]
        cursor = 364 + proof_length
        count = certificate[cursor]
        cursor += 1
        attestations = []
        for _ in range(count):
            w = certificate[cursor:cursor + 274]
            cursor += 274
            u = lambda begin, end: int.from_bytes(w[begin:end], 'big')
            attestations.append((u(0, 2), u(2, 6), u(6, 14), '0x' + w[14:34].hex(), u(34, 42), w[42:74], w[74:106], w[106:138], u(138, 146), w[146:178],
                                 w[178] == 1, w[179] == 1, w[180], u(181, 189), s.to_checksum_address(w[189:209]), w[209:241], w[241:273], w[273]))
        threshold = certificate[cursor]
        self.assertEqual((count, threshold), (int(case['signers']), int(case['threshold'])))
        self.assertEqual(int.from_bytes(certificate[cursor + 1:cursor + 3], 'big'), len(certificate) - cursor - 3)
        return case, context, tuple(header), proof, attestations, threshold, certificate[:cursor + 1]

    def test_canonical_hash_and_vector_hash(self):
        self.assertEqual(s.checkpoint_hash(self.header, s.raw(self.vector['certificate']['validity_proof'])), self.digest)
        case, _, header, proof, _, _, _ = self.anchor_vector()
        self.assertEqual(s.header_encode(header).hex(), case['header'])
        self.assertEqual(s.checkpoint_hash(header, proof).hex(), case['checkpoint_id'])

    def test_anchor_certificate_and_submit_calldata_match_the_c_payload(self):
        case, context, header, proof, attestations, threshold, prefix = self.anchor_vector()
        self.assertEqual(s.raw(s.ANCHOR, 20).hex(), context['settlement_contract'])
        certificate = s.certificate_encode(header, proof, attestations, threshold)
        self.assertEqual(certificate, prefix + bytes(2))
        digest = bytes.fromhex(case['checkpoint_id'])
        for a in attestations:
            s.validate_attestation(a, header, digest, int(context['paxeer_chain_id']), s.ANCHOR, int(context['maximum_attestation_delay_ms']))
            with self.assertRaises(ValueError):
                s.validate_attestation(a, header, digest, int(context['paxeer_chain_id']), self.bond, int(context['maximum_attestation_delay_ms']))
        signature = bytes.fromhex(case['header_signature'])
        data = s.raw(s.submit_calldata(header, signature, proof, attestations, threshold))
        self.assertEqual(data[:4], s.keccak(text='submitCheckpoint(bytes,bytes,bytes)')[:4])
        self.assertEqual(s.decode(('bytes', 'bytes', 'bytes'), data[4:]), (bytes.fromhex(case['header']), signature, certificate))
        self.assertEqual(int.from_bytes(data[4:36], 'big'), 96)
        self.assertEqual(int.from_bytes(data[36:68], 'big'), 96 + 32 + 384)
        self.assertEqual(int.from_bytes(data[68:100], 'big'), 96 + 32 + 384 + 32 + 64)
        with self.assertRaises(ValueError):
            s.submit_calldata(header, bytes(64), proof, attestations, threshold)
        with self.assertRaises(ValueError):
            s.certificate_encode(header, proof, attestations, len(attestations) + 1)

    def test_signatures_and_topics_are_the_precompile_abi(self):
        entries = json.loads((ROOT / 'precompiles/layerxanchor/abi.json').read_text())

        def kind(item):
            return '(' + ','.join(kind(c) for c in item['components']) + ')' if item['type'] == 'tuple' else item['type']

        def signature(entry):
            return entry['name'] + '(' + ','.join(kind(i) for i in entry['inputs']) + ')'

        functions = {e['name']: e for e in entries if e['type'] == 'function'}
        events = {e['name']: e for e in entries if e['type'] == 'event'}
        self.assertEqual(signature(functions['submitCheckpoint']), s.SUBMIT)
        self.assertEqual(kind(functions['checkpoint']['outputs'][0]), s.CHECKPOINT_TUPLE)
        self.assertEqual(kind(functions['guarantor']['outputs'][0]), s.GUARANTOR_TUPLE)
        self.assertEqual([functions[name]['stateMutability'] for name in ('registerGuarantor', 'increaseBond', 'openChallenge')], ['payable'] * 3)
        for name in ('registerGuarantor', 'increaseBond', 'openChallenge', 'submitEquivocation', 'finalize', 'statusOf', 'threshold', 'guarantor', 'checkpoint'):
            self.assertIn("'" + signature(functions[name]) + "'", (ROOT / 'cmd/layerx-guarantor/settlement.py').read_text())
        for name, value in [('CheckpointSubmitted', s.SUBMITTED_EVENT), ('CheckpointFinalized', s.FINALIZED_EVENT), ('GuarantorRegistered', s.REGISTERED_EVENT),
                            ('GuarantorActivated', s.ACTIVATED_EVENT), ('BondIncreased', s.BOND_EVENT), ('UnbondBegun', s.UNBOND_EVENT), ('GuarantorSlashed', s.SLASHED_EVENT)]:
            self.assertEqual(s.topic(signature(events[name])), value)
        self.assertEqual(s.UNIT_WEI, 10 ** 12)

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
        tx = '0x' + '34' * 32
        block_hash = '0x' + '56' * 32
        indexed = ['0x' + s.encode(['uint64'], [self.header[3]]).hex(), '0x' + self.digest.hex()]
        common = {'address': s.ANCHOR, 'transactionHash': tx, 'blockHash': block_hash, 'blockNumber': '0x1', 'removed': False}
        submitted = dict(common, topics=[s.SUBMITTED_EVENT] + indexed, data='0x' + s.encode(['bytes32', 'bytes32', 'uint8'], [self.header[7], self.header[9], 3]).hex())
        finalized = dict(common, topics=[s.FINALIZED_EVENT] + indexed, data='0x' + s.encode(['bytes32', 'bytes32'], [self.header[7], self.header[9]]).hex())
        receipt = {'status': '0x1', 'transactionHash': tx, 'to': s.ANCHOR, 'blockHash': block_hash, 'blockNumber': '0x1', 'logs': [submitted, finalized]}
        self.assertEqual(s.validate_receipt(receipt, tx, self.digest, self.header, 3), (1, True))
        self.assertEqual(s.validate_receipt(dict(receipt, logs=[submitted]), tx, self.digest, self.header, 3), (1, False))
        for key, value in [('removed', True), ('data', '0x' + bytes(96).hex()), ('transactionHash', '0x' + 'ff' * 32), ('blockHash', '0x' + 'ff' * 32), ('blockNumber', '0x2')]:
            for position in (0, 1):
                changed = copy.deepcopy(receipt)
                changed['logs'][position][key] = value
                with self.subTest(key=key, position=position), self.assertRaises(ValueError):
                    s.validate_receipt(changed, tx, self.digest, self.header, 3)
        for logs in [[], [finalized], [submitted, submitted], [submitted, finalized, finalized]]:
            with self.assertRaises(ValueError):
                s.validate_receipt(dict(receipt, logs=logs), tx, self.digest, self.header, 3)
        with self.assertRaises(ValueError):
            s.validate_receipt(receipt, tx, self.digest, self.header, 2)
        with self.assertRaises(ValueError):
            s.validate_receipt(dict(receipt, status='0x0'), tx, self.digest, self.header, 3)
        with self.assertRaises(ValueError):
            s.validate_receipt(dict(receipt, to='0x' + '12' * 20), tx, self.digest, self.header, 3)

    def test_anchor_checkpoint_record_comparison(self):
        h = self.header
        record = (h[3], self.digest, bytes(32), h[2], h[4], h[5], h[6], h[7], h[9], h[11], h[14], h[13], 2, 3, 31, 0, 9, 9)
        s.require_checkpoint(record, self.digest, h, 3)
        for index in (1, 3, 4, 5, 6, 7, 8, 9, 10, 11, 13):
            changed = list(record)
            changed[index] = bytes([1]) * 32 if isinstance(changed[index], bytes) else changed[index] + 1
            with self.subTest(index=index), self.assertRaises(ValueError):
                s.require_checkpoint(tuple(changed), self.digest, h, 3)
        with self.assertRaises(ValueError):
            s.require_checkpoint(None, self.digest, h, 3)

    def test_real_submitter_transaction_signing(self):
        account = s.Account.create()
        transaction = {'chainId': 31337, 'nonce': 0, 'to': s.to_checksum_address(self.bond), 'value': 0, 'gas': 100000, 'gasPrice': 1000000000, 'data': s.calldata('increaseBond(bytes32)', ('bytes32',), (bytes([7]) * 32,))}
        signed = account.sign_transaction(transaction)
        self.assertEqual(s.Account.recover_transaction(signed.raw_transaction), account.address)
        self.assertEqual(s.keccak(bytes(signed.raw_transaction)), signed.hash)

    def test_wire_membership_and_registration(self):
        member = {'guarantor_id': '0x' + self.attestations[0][7].hex(), 'signer': self.attestations[0][14], 'bonded_active': True, 'bond_amount': 1000, 'joined_epoch': 1, 'authorization_version': 2}
        wire = s.wire_encode('membership', {'version': 4, 'threshold': 2, 'maximum_attestation_delay_ms': 3600000, 'minimum_bond': 100, 'block_number': 4096, 'governance_sequence': 3, 'custodied_value': 10000, 'minimum_bond_bps': 100, 'members': [member]})
        self.assertEqual(len(wire), 161)
        self.assertEqual(struct.unpack('>QIQI', wire[:24]), (4, 2, 3600000, 1))
        self.assertEqual(int.from_bytes(wire[24:40], 'big'), 100)
        self.assertEqual(struct.unpack('>QQ', wire[40:56]), (4096, 3))
        self.assertEqual(int.from_bytes(wire[56:72], 'big'), 10000)
        self.assertEqual(struct.unpack('>I', wire[72:76])[0], 100)
        self.assertEqual(wire[76:108], self.attestations[0][7])
        self.assertEqual(int.from_bytes(wire[129:145], 'big'), 1000)
        registration = s.wire_encode('register', {'already_registered': True, 'transaction_id': '0x' + '12' * 32, 'observed_block_number': 5, 'observed_at_ms': 1000000, 'set_version': 4})
        self.assertEqual(len(registration), 57)
        self.assertEqual(struct.unpack('>QQQ', registration[33:]), (5, 1000000, 4))
        funding = s.wire_encode('deposit', {'guarantor_id': '0x' + self.attestations[0][7].hex(), 'transaction_id': '0x' + '34' * 32, 'observed_block_number': 4097, 'observed_at_ms': 1700000000000, 'membership_version': 5, 'amount': 250, 'total_bond': 1250})
        self.assertEqual(len(funding), 120)
        self.assertEqual(funding[:32], self.attestations[0][7])
        self.assertEqual(funding[32:64], bytes.fromhex('34' * 32))
        self.assertEqual(struct.unpack('>QQQ', funding[64:88]), (4097, 1700000000000, 5))
        self.assertEqual(int.from_bytes(funding[88:104], 'big'), 250)
        self.assertEqual(int.from_bytes(funding[104:120], 'big'), 1250)

    def test_configuration_real_vector_public_keys_and_environment(self):
        document = json.loads((ROOT / 'contracts/config/checkpoint-settlement.json').read_text())
        domain = copy.deepcopy(document['settlement_domains']['vectors'])
        domain['guarantor_bond'] = s.ANCHOR
        domain['settlement_contract'] = s.ANCHOR
        domain['minimum_bond'] = 1_000_000
        domain['maximum_attestation_delay_ms'] = 3_600_000
        document['settlement_domains']['beta'] = domain
        environment = {'LAYERX_NODE_PAXEER_CHAIN_ID': str(domain['paxeer_chain_id']), 'LAYERX_NODE_SETTLEMENT_CONTRACT': s.ANCHOR, 'LAYERX_NODE_CHECKPOINT_REGISTRY': s.ANCHOR}
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
                os.environ['LAYERX_NODE_PAXEER_CHAIN_ID'] = str(domain['paxeer_chain_id'])
                for key in ('guarantor_bond', 'settlement_contract'):
                    solidity = copy.deepcopy(document)
                    solidity['settlement_domains']['beta'][key] = '0x' + '23' * 20
                    path.write_text(json.dumps(solidity))
                    with self.subTest(key=key), self.assertRaises(ValueError):
                        s.configuration(request)
                for key in ('minimum_bond', 'maximum_attestation_delay_ms'):
                    missing = copy.deepcopy(document)
                    del missing['settlement_domains']['beta'][key]
                    path.write_text(json.dumps(missing))
                    with self.subTest(key=key), self.assertRaises(ValueError):
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


class NativePublicationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        spec = importlib.util.spec_from_file_location('native_publication', ROOT / 'cmd/layerx-guarantor/publication.py')
        cls.p = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.p)

    def test_c_witness_vectors_and_noncanonical_paths(self):
        for vector in json.loads((ROOT / 'contracts/config/native-state-proofs.json').read_text())['vectors']:
            root, proof = s.raw(vector['root']), s.raw(vector['proof'])
            self.p.witness(vector['proof'], root)
            for position in (0, 3, 7, len(proof) - 1):
                altered = bytearray(proof)
                altered[position] ^= 1
                with self.subTest(position=position), self.assertRaises(ValueError):
                    self.p.witness(self.p.hx(altered), root)
            for altered in (proof[:-1], proof + b'\0'):
                with self.assertRaises(ValueError):
                    self.p.witness(self.p.hx(altered), root)
            with self.assertRaises(ValueError):
                self.p.witness(vector['proof'], bytes(32))

    def test_native_withdrawal_strict_record_and_network(self):
        vector = json.loads((ROOT / 'contracts/config/native-withdrawal-proof.json').read_text())
        fact = self.p.withdrawal_fact(vector['proof'], s.raw(vector['root']), 7)
        self.assertEqual(len(fact['identity']), 32)
        self.assertEqual(fact['amount'], (25).to_bytes(16, 'big'))
        self.assertEqual(fact['anchor'], bytes([3]) + bytes(31))
        with self.assertRaises(ValueError):
            self.p.withdrawal_fact(vector['proof'], s.raw(vector['root']), 8)
        with self.assertRaises(ValueError):
            self.p.balance_fact(vector['proof'], s.raw(vector['root']))

    def test_independent_ed25519_authorities_and_mutations(self):
        from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
        from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat
        owner, authority = Ed25519PrivateKey.generate(), Ed25519PrivateKey.generate()
        public = owner.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
        message = b'LX:SETTLE:RECIPIENT:v1\0' + (77).to_bytes(4, 'big') + bytes([1]) * 32 + bytes([2]) * 32 + bytes([3]) * 20 + bytes([4]) * 32
        signed = owner.sign(message)
        self.p.signature(public, message, signed)
        with self.assertRaises(ValueError):
            self.p.signature(public, message, authority.sign(message))
        for index in range(len(signed)):
            altered = bytearray(signed)
            altered[index] ^= 1
            with self.assertRaises(ValueError):
                self.p.signature(public, message, bytes(altered))
        for index in (20, 24, 56, 88, len(message) - 1):
            altered = bytearray(message)
            altered[index] ^= 1
            with self.assertRaises(ValueError):
                self.p.signature(public, bytes(altered), signed)

    def test_atomic_evidence_retry_and_authorization_file_refusals(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'checkpoint.json'
            abandoned = Path(str(path) + '.tmp')
            abandoned.write_bytes(b'interrupted write')
            self.p.atomic_json(path, {'version': 2})
            self.p.atomic_json(path, {'version': 2, 'complete': True})
            self.assertEqual(self.p.read_authorizations(path), {'version': 2, 'complete': True})
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)
            link = Path(directory) / 'linked'
            link.symlink_to(path)
            with self.assertRaises(OSError):
                self.p.read_authorizations(link)
            link.unlink()
            os.link(path, link)
            with self.assertRaises(ValueError):
                self.p.read_authorizations(path)


if __name__ == '__main__':
    unittest.main()
