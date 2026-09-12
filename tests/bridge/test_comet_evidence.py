import argparse
import base64
import copy
import json
import os
from pathlib import Path
import subprocess
import unittest

from comet_credit import verifier


class RealCometEvidence(unittest.TestCase):
    evidence = None

    def request(self):
        return copy.deepcopy(self.evidence['requests'][0])

    def refused(self, change):
        request = self.request()
        change(request)
        with self.assertRaises(ValueError):
            verifier(request)

    def test_both_complete_real_proofs(self):
        self.assertEqual(len(self.evidence['requests']), 2)
        for request, expected in zip(self.evidence['requests'], self.evidence['results'], strict=True):
            self.assertEqual(verifier(request), expected)

    def test_cli_emits_one_json_document(self):
        root = Path(__file__).resolve().parents[2]
        binary = os.environ.get('LAYERX_CUSTODY_PROOF_BIN', str(root/'build/bin/layerx-custody-proof'))
        command = [binary, '--history-state', os.environ['LAYERX_CUSTODY_HISTORY_STATE'],
                   '--attestor-key', os.environ['LAYERX_CUSTODY_HISTORY_KEY']]
        completed = subprocess.run(command, input=json.dumps(self.request()).encode(),
                                   capture_output=True, timeout=120, check=False)
        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors='replace'))
        self.assertTrue(completed.stdout.startswith(b'{'))
        self.assertTrue(completed.stdout.endswith(b'}\n'))
        self.assertEqual(json.loads(completed.stdout), self.evidence['results'][0])

    def test_genesis_and_chain_binding(self):
        self.refused(lambda r: r['expected'].__setitem__('genesis_sha256', '0x'+'01'*32))
        self.refused(lambda r: r['expected'].__setitem__('comet_chain_id', 'untrusted-125'))
        self.refused(lambda r: r['expected'].__setitem__('chain_id', 126))
        self.refused(lambda r: r['bundle'].__setitem__('genesis', base64.b64encode(b'{}').decode()))

    def test_full_history_and_ancestry(self):
        complete = self.request()
        complete['bundle']['history'] = verifier(complete | {'operation': 'export'})
        state = os.environ.pop('LAYERX_CUSTODY_HISTORY_STATE')
        authority = os.environ.pop('LAYERX_CUSTODY_HISTORY_KEY')
        try:
            self.assertEqual(verifier(complete)['deposit_id'], complete['expected']['deposit_id'])
            changes = [lambda r: r['bundle']['history'].pop(0),
                       lambda r: r['bundle']['history'].pop(1),
                       lambda r: r['bundle']['history'].append(r['bundle']['history'][-1]),
                       lambda r: r['bundle']['history'][1].__setitem__('commit', r['bundle']['history'][0]['commit']),
                       lambda r: r['bundle']['history'][1]['commit']['signed_header']['header']['last_block_id'].__setitem__('hash', '01'*32)]
            for change in changes:
                altered = copy.deepcopy(complete)
                change(altered)
                with self.assertRaises(ValueError):
                    verifier(altered)
        finally:
            os.environ['LAYERX_CUSTODY_HISTORY_STATE'] = state
            os.environ['LAYERX_CUSTODY_HISTORY_KEY'] = authority

    def test_header_commit_and_validator_authentication(self):
        self.refused(lambda r: r['bundle']['history'][0]['commit'].__setitem__('canonical', False))
        self.refused(lambda r: r['bundle']['history'][0]['commit']['signed_header']['header'].__setitem__('app_hash', '01'*32))
        def changed_power(request):
            validator = request['bundle']['history'][0]['validators'][0]['validators'][0]
            validator['voting_power'] = str(int(validator['voting_power'])+1)

        self.refused(changed_power)
        self.refused(lambda r: r['bundle']['history'][0]['validators'][0]['validators'][0].__setitem__('address', '01'*20))
        self.refused(lambda r: r['bundle']['history'][0]['validators'][0].__setitem__('total', '1001'))

        def damaged_signature(request):
            signatures = request['bundle']['history'][0]['commit']['signed_header']['commit']['signatures']
            signature = next(value for value in signatures if value.get('signature'))
            data = bytearray(base64.b64decode(signature['signature'], validate=True))
            data[0] ^= 1
            signature['signature'] = base64.b64encode(data).decode()

        self.refused(damaged_signature)
        self.refused(lambda r: r['bundle']['history'][0]['commit']['signed_header']['commit'].__setitem__('signatures', []))

    def test_exact_state_query_identity(self):
        for name in ('code', 'code_hash', 'deposit'):
            with self.subTest(name=name):
                self.refused(lambda r: r['bundle']['state'][0][name].__setitem__('height', '999999'))
                self.refused(lambda r: r['bundle']['state'][0][name].__setitem__('code', 1))
                self.refused(lambda r: r['bundle']['state'][0][name].__setitem__('key', base64.b64encode(b'wrong').decode()))
                self.refused(lambda r: r['bundle']['state'][0][name].__setitem__('value', base64.b64encode(bytes(32)).decode()))

    def test_composed_application_root_proofs(self):
        for name in ('code', 'code_hash', 'deposit'):
            with self.subTest(name=name):
                self.refused(lambda r: r['bundle']['state'][0][name].__setitem__('proofOps', None))
                proof_name = 'proofOps' if 'proofOps' in self.request()['bundle']['state'][0][name] else 'proof_ops'
                self.refused(lambda r: r['bundle']['state'][0][name].__setitem__(proof_name, None))
                self.refused(lambda r: r['bundle']['state'][0][name][proof_name]['ops'].pop())
                self.refused(lambda r: r['bundle']['state'][0][name][proof_name]['ops'].reverse())
                self.refused(lambda r: r['bundle']['state'][0][name][proof_name]['ops'][1].__setitem__('key', base64.b64encode(b'bank').decode()))
                self.refused(lambda r: r['bundle']['state'][0][name][proof_name]['ops'][0].__setitem__('type', 'unverified'))

                def damaged_proof(request):
                    op = request['bundle']['state'][0][name][proof_name]['ops'][0]
                    data = bytearray(base64.b64decode(op['data'], validate=True))
                    data[-1] ^= 1
                    op['data'] = base64.b64encode(data).decode()

                self.refused(damaged_proof)

    def test_deposit_preimage_and_runtime_binding(self):
        self.refused(lambda r: r['expected'].__setitem__('deposit_id', '0x'+'01'*32))
        self.refused(lambda r: r['expected'].__setitem__('deposit_slot', '0x'+'01'*32))
        self.refused(lambda r: r['expected'].__setitem__('vault', '0x'+'01'*20))
        self.refused(lambda r: r['expected'].__setitem__('runtime_sha256', '0x'+'01'*32))
        self.refused(lambda r: r['bundle']['state'][0].pop('deposit'))

    def test_finality_and_point_bounds(self):
        self.refused(lambda r: r['expected'].__setitem__('confirmations', 8193))
        self.refused(lambda r: r['bundle'].__setitem__('state_height', 1))
        self.refused(lambda r: r['bundle'].__setitem__('finalized_height', r['bundle']['state_height']-1))
        self.refused(lambda r: r['bundle']['state'][0].__setitem__('height', r['bundle']['state_height']+1))
        self.refused(lambda r: r['bundle']['state'].pop())


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--evidence', type=Path, required=True)
    parser.add_argument('--history-state', required=True)
    parser.add_argument('--attestor-key', required=True)
    args, remaining = parser.parse_known_args()
    os.environ['LAYERX_CUSTODY_HISTORY_STATE'] = args.history_state
    os.environ['LAYERX_CUSTODY_HISTORY_KEY'] = args.attestor_key
    RealCometEvidence.evidence = json.loads(args.evidence.read_bytes())
    require_deposit = RealCometEvidence.evidence['requests'][0]['expected']['deposit_id']
    if not require_deposit:
        raise ValueError('real deposit membership evidence required')
    unittest.main(argv=[__file__, *remaining])
