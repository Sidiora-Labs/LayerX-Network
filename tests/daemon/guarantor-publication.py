#!/usr/bin/env python3
import copy
import hashlib
import importlib.util
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / 'tests/fixtures/custody/paxeer-light-v1'
RETIRED = ROOT / 'tests/fixtures/custody/paxeer-state-v2'


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


s = load('publication_settlement', ROOT / 'cmd/layerx-guarantor/settlement.py')
p = load('publication_codec', ROOT / 'cmd/layerx-guarantor/publication.py')


def sha(value):
    return hashlib.sha256(value).digest()


def node(left, right):
    return sha(b'LXP/v1/state-node\0' + left + right)


def merkle(leaves, index):
    level, path = list(leaves), []
    while len(level) > 1:
        partner = index ^ 1
        path.append(level[partner] if partner < len(level) else level[index])
        level = [node(level[i], level[i + 1] if i + 1 < len(level) else level[i])
                 for i in range(0, len(level), 2)]
        index //= 2
    return path, level[0]


def encode(module, key, value, leaves, index, modules, module_index):
    leaf_path, subtree = merkle(leaves, index)
    modules = list(modules)
    modules[module_index] = p.state_leaf(module.to_bytes(2, 'big'), subtree)
    module_path, root = merkle(modules, module_index)
    wire = ((2).to_bytes(2, 'big') + module.to_bytes(2, 'big') +
            len(key).to_bytes(4, 'big') + key + len(value).to_bytes(4, 'big') + value +
            index.to_bytes(4, 'big') + len(leaves).to_bytes(4, 'big') +
            len(leaf_path).to_bytes(1, 'big') + b''.join(leaf_path) +
            len(modules).to_bytes(4, 'big') +
            len(module_path).to_bytes(1, 'big') + b''.join(module_path))
    return p.hx(wire), root


class LightProfilePublicationTests(unittest.TestCase):
    def setUp(self):
        self.profile = (FIXTURES / 'custody.profile').read_bytes()
        self.payload = (FIXTURES / 'custody.credit').read_bytes()
        self.credit = self.payload[:p.CREDIT_BYTES]
        self.nullifier = bytes.fromhex((FIXTURES / 'custody.credit.nullifier').read_text().strip())
        self.network = int.from_bytes(self.profile[201:205], 'big')
        self.protocol = int.from_bytes(self.profile[205:207], 'big')
        self.assertEqual((len(self.profile), self.profile[:5]), (223, b'LXBC3'))
        self.assertEqual(self.credit[:5], b'LXDC3')
        self.assertEqual(self.nullifier, sha(b'LX:DEPOSIT:NULLIFIER:v1' + self.credit[43:75]))

    def facts(self, profile=None, credit=None, nullifier=None):
        profile = self.profile if profile is None else profile
        credit = self.credit if credit is None else credit
        nullifier = self.nullifier if nullifier is None else nullifier
        others = [p.state_leaf(index.to_bytes(2, 'big'), sha(b'LXP/v1/state-subtree\0' +
                  index.to_bytes(2, 'big'))) for index in range(9)]
        key = b'deposit-nullifier:' + nullifier
        leaves = [p.state_leaf(p.PROFILE_KEY, profile), p.state_leaf(key, credit)]
        profile_witness, root = encode(8, p.PROFILE_KEY, profile, leaves, 0, others, 8)
        credit_witness, credit_root = encode(8, key, credit, leaves, 1, others, 8)
        self.assertEqual(root, credit_root)
        header = [self.protocol, self.network, 7, 11, 1, 1000] + [bytes(32)] * 7 + [1, bytes(32)]
        header[7] = root
        return header, {'balances': [], 'withdrawals': [], 'deposits': [credit_witness],
                        'profile': profile_witness}

    def request(self, facts):
        return dict(chain_id=125, native_facts=facts)

    def run_native(self, header, facts):
        return p.native_request(s, self.request(facts), header, bytes(32))

    def test_real_light_profile_and_credit_accepted(self):
        header, facts = self.facts()
        balances, withdrawals, deposits, profile = self.run_native(header, facts)
        self.assertEqual((balances, withdrawals), ([], []))
        self.assertEqual(profile, self.profile)
        self.assertEqual(len(deposits), 1)
        deposit = deposits[0]
        self.assertEqual(deposit['identity'], self.credit[43:75])
        self.assertEqual(deposit['asset'], self.profile[97:129])
        self.assertEqual(deposit['beneficiary'], self.credit[107:139])
        self.assertEqual(deposit['payer'], self.credit[171:191])
        self.assertEqual(deposit['amount'], self.credit[191:207])
        self.assertEqual(deposit['nonce'], int.from_bytes(self.credit[207:215], 'big'))
        self.assertEqual(p.deposit_identifier(self.profile, self.credit), deposit['identity'])

    def test_retired_attestor_profile_refused(self):
        retired = (RETIRED / 'custody.profile').read_bytes()
        retired_credit = (RETIRED / 'custody.credit').read_bytes()
        self.assertEqual((len(retired), len(retired_credit)), (207, 427))
        self.assertIn(retired[:5], (b'LXBC1', b'LXBC2'))
        header, facts = self.facts(profile=retired)
        with self.assertRaises(ValueError) as caught:
            self.run_native(header, facts)
        self.assertEqual(str(caught.exception), 'native custody profile')

    def test_retired_attestor_credit_refused(self):
        retired_credit = (RETIRED / 'custody.credit').read_bytes()
        header, facts = self.facts(credit=retired_credit)
        with self.assertRaises(ValueError) as caught:
            self.run_native(header, facts)
        self.assertEqual(str(caught.exception), 'native custody credit')

    def test_credit_head_with_bundle_appended_refused(self):
        header, facts = self.facts(credit=self.payload)
        with self.assertRaises(ValueError) as caught:
            self.run_native(header, facts)
        self.assertEqual(str(caught.exception), 'native custody credit')

    def test_altered_amount_breaks_deposit_identifier(self):
        altered = bytearray(self.credit)
        altered[206] ^= 1
        header, facts = self.facts(credit=bytes(altered))
        with self.assertRaises(ValueError) as caught:
            self.run_native(header, facts)
        self.assertEqual(str(caught.exception), 'native deposit identifier')

    def test_altered_payer_breaks_deposit_identifier(self):
        altered = bytearray(self.credit)
        altered[171] ^= 1
        header, facts = self.facts(credit=bytes(altered))
        with self.assertRaises(ValueError) as caught:
            self.run_native(header, facts)
        self.assertEqual(str(caught.exception), 'native deposit identifier')

    def test_nullifier_key_must_commit_to_the_deposit(self):
        header, facts = self.facts(nullifier=sha(b'LX:DEPOSIT:NULLIFIER:v1' + bytes(32)))
        with self.assertRaises(ValueError) as caught:
            self.run_native(header, facts)
        self.assertEqual(str(caught.exception), 'native deposit nullifier')

    def test_credit_profile_hash_binding(self):
        altered = bytearray(self.profile)
        altered[96] ^= 1
        header, facts = self.facts(profile=bytes(altered))
        with self.assertRaises(ValueError) as caught:
            self.run_native(header, facts)
        self.assertEqual(str(caught.exception), 'native custody domain')

    def test_profile_module_identity_pinned(self):
        altered = bytearray(self.profile)
        altered[33] ^= 1
        header, facts = self.facts(profile=bytes(altered))
        with self.assertRaises(ValueError) as caught:
            self.run_native(header, facts)
        self.assertEqual(str(caught.exception), 'native custody profile identity')

    def test_profile_reserve_account_pinned(self):
        altered = bytearray(self.profile)
        altered[160] ^= 1
        header, facts = self.facts(profile=bytes(altered))
        with self.assertRaises(ValueError) as caught:
            self.run_native(header, facts)
        self.assertEqual(str(caught.exception), 'native custody profile identity')

    def test_profile_network_must_match_the_batch_header(self):
        header, facts = self.facts()
        header[1] = self.network + 1
        with self.assertRaises(ValueError) as caught:
            self.run_native(header, facts)
        self.assertEqual(str(caught.exception), 'native custody profile domain')

    def test_profile_comet_chain_padding_enforced(self):
        altered = bytearray(self.profile)
        altered[200] = 0x41
        header, facts = self.facts(profile=bytes(altered))
        with self.assertRaises(ValueError) as caught:
            self.run_native(header, facts)
        self.assertEqual(str(caught.exception), 'native custody Comet chain identifier')

    def test_profile_trusted_time_bounds(self):
        altered = bytearray(self.profile)
        altered[215:223] = (0).to_bytes(8, 'big')
        header, facts = self.facts(profile=bytes(altered))
        with self.assertRaises(ValueError) as caught:
            self.run_native(header, facts)
        self.assertEqual(str(caught.exception), 'native custody profile trust state')

    def test_credit_state_height_must_precede_the_signed_header(self):
        altered = bytearray(self.credit)
        altered[287:295] = int.from_bytes(self.credit[215:223], 'big').to_bytes(8, 'big')
        header, facts = self.facts(credit=bytes(altered))
        with self.assertRaises(ValueError) as caught:
            self.run_native(header, facts)
        self.assertEqual(str(caught.exception), 'native custody light-client evidence')

    def test_credit_proof_kind_pinned_to_the_light_client(self):
        altered = bytearray(self.credit)
        altered[359:363] = (1).to_bytes(4, 'big')
        header, facts = self.facts(credit=bytes(altered))
        with self.assertRaises(ValueError) as caught:
            self.run_native(header, facts)
        self.assertEqual(str(caught.exception), 'native custody light-client evidence')

    def test_credit_asset_must_be_the_profile_asset(self):
        altered = bytearray(self.credit)
        altered[75] ^= 1
        header, facts = self.facts(credit=bytes(altered))
        with self.assertRaises(ValueError) as caught:
            self.run_native(header, facts)
        self.assertEqual(str(caught.exception), 'native custody credit fields')

    def test_deposit_without_profile_refused(self):
        header, facts = self.facts()
        facts = copy.deepcopy(facts)
        facts['profile'] = None
        with self.assertRaises(ValueError) as caught:
            self.run_native(header, facts)
        self.assertEqual(str(caught.exception), 'native custody profile absent')

    def test_witness_must_prove_against_the_batch_state_root(self):
        header, facts = self.facts()
        header[7] = sha(header[7])
        with self.assertRaises(ValueError) as caught:
            self.run_native(header, facts)
        self.assertEqual(str(caught.exception), 'native witness root or trailing data')


if __name__ == '__main__':
    unittest.main(verbosity=2)
