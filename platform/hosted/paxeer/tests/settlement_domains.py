import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[4]
spec = importlib.util.spec_from_file_location('settlement_domain', ROOT/'platform/hosted/paxeer/settlement-domain.py')
helper = importlib.util.module_from_spec(spec)
spec.loader.exec_module(helper)


class SettlementDomains(unittest.TestCase):
    def test_domain_writer_preserves_vectors_and_generator_binds_signatures(self):
        original = json.loads((ROOT/'contracts/config/checkpoint-settlement.json').read_text())
        with tempfile.TemporaryDirectory() as work:
            work = Path(work)
            settlement = work/'settlement.json'
            settlement.write_text(json.dumps(original))
            domain = dict(original['settlement_domains']['vectors'])
            domain['paxeer_chain_id'] = 125
            domain['guarantor_bond'] = '0x0000000000000000000000000000000000000125'
            helper.write_domain(settlement, 'beta', domain)
            document = json.loads(settlement.read_text())
            self.assertEqual(original['settlement_domains']['vectors'], document['settlement_domains']['vectors'])
            with self.assertRaises(SystemExit):
                helper.write_domain(settlement, 'vectors', domain)
            keys = work/'keys.json'
            keys.write_text(json.dumps(['0x1', '0x2', '0x3']))
            subprocess.run(['python3', str(ROOT/'tests/vectors/checkpoint/generate.py'),
                            '--domain', 'beta', '--settlement', str(settlement),
                            '--keys-file', str(keys), '--output', str(work/'beta')],
                           check=True, stdout=subprocess.DEVNULL)
            vector = json.loads((work/'beta/fresh.json').read_text())
            canonical = json.loads((ROOT/'tests/vectors/checkpoint/fresh.json').read_text())
            self.assertEqual(vector['settlement_domain'], 'beta')
            self.assertNotEqual(vector['attestations'][0]['digest'], canonical['attestations'][0]['digest'])
            self.assertNotEqual(vector['attestations'][0]['signature'], canonical['attestations'][0]['signature'])
            domain['protocol_version'] = 3
            helper.write_domain(settlement, 'beta3', domain)
            subprocess.run(['python3', str(ROOT/'tests/vectors/checkpoint/generate.py'),
                            '--domain', 'beta3', '--settlement', str(settlement),
                            '--keys-file', str(keys), '--output', str(work/'beta3')],
                           check=True, stdout=subprocess.DEVNULL)
            versioned = json.loads((work/'beta3/fresh.json').read_text())
            self.assertEqual(versioned['header']['protocol_version'], 3)
            self.assertNotEqual(versioned['attestations'][0]['digest'], vector['attestations'][0]['digest'])
            self.assertNotEqual(versioned['attestations'][0]['signature'], vector['attestations'][0]['signature'])
            self.assertEqual(json.loads(settlement.read_text())['settlement_domains']['vectors'],
                             original['settlement_domains']['vectors'])
            self.assertEqual(json.loads(settlement.read_text())['protocol_version'], 2)
            domain['protocol_version'] = 4
            with self.assertRaises(SystemExit):
                helper.write_domain(settlement, 'unsupported', domain)


if __name__ == '__main__':
    unittest.main()
