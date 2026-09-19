import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import yaml

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
spec = importlib.util.spec_from_file_location('material', HERE / 'material.py')
material = importlib.util.module_from_spec(spec)
spec.loader.exec_module(material)


class MaterialTests(unittest.TestCase):
    def test_generated_bootstrap_and_protected_inputs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / 'human'
            root.mkdir(mode=0o700)
            for name in ('components', 'kms', 'config', 'agent-config'):
                (root / name).mkdir(mode=0o700)
            (root.parent / 'receipt-authority-replica-id').write_text('6c61796572782d626574612d726563656970742d617574686f726974792d3031')
            subprocess.run([sys.executable, str(HERE / 'material.py'), str(root), '402', '31337', ''], check=True)
            self.assertEqual((root / 'agent-config/LAYERX_AGENT_MODE').read_text(), 'human-owner')
            for path in root.rglob('*'):
                if path.is_file():
                    self.assertEqual(path.stat().st_mode & 0o777, 0o600)
            protected = Path(directory) / 'policy.json'
            protected.write_text('{}')
            protected.chmod(0o600)
            self.assertEqual(material.protected_json(protected), {})
            alias = Path(directory) / 'alias.json'
            alias.symlink_to(protected)
            with self.assertRaises(ValueError):
                material.protected_json(alias)
            protected.chmod(0o640)
            with self.assertRaises(ValueError):
                material.protected_json(protected)
            protected.chmod(0o600)
            os.link(protected, Path(directory) / 'hardlink.json')
            with self.assertRaises(ValueError):
                material.protected_json(protected)
            with self.assertRaises(FileNotFoundError):
                material.assemble_policy(Path(directory), Path(directory) / 'missing-deployment.json',
                                         Path(directory) / 'missing-registry.json',
                                         Path(directory) / 'output.json', 402, 31337)
            self.assertFalse((Path(directory) / 'output.json').exists())

    def test_passkey_relying_party_follows_the_deployed_web_origin(self):
        self.assertEqual(material.passkey_relying_party(''),
                         ('human.testnet.layerx.network', 'https://human.testnet.layerx.network'))
        self.assertEqual(material.passkey_relying_party('https://human.beta.layerx.example'),
                         ('human.beta.layerx.example', 'https://human.beta.layerx.example'))
        for refused in ('http://human.testnet.layerx.network', 'https://localhost:19457',
                        'https://127.0.0.1', 'https://human.testnet.layerx.network/',
                        'https://Human.Testnet.Layerx.Network', 'https://human.testnet.layerx.network?a=1'):
            with self.assertRaises(ValueError):
                material.passkey_relying_party(refused)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / 'human'
            root.mkdir(mode=0o700)
            for name in ('components', 'kms', 'config', 'agent-config'):
                (root / name).mkdir(mode=0o700)
            (root.parent / 'receipt-authority-replica-id').write_text('6c61796572782d626574612d726563656970742d617574686f726974792d3031')
            subprocess.run([sys.executable, str(HERE / 'material.py'), str(root), '402', '31337', '',
                            'https://human.beta.layerx.example'], check=True)
            self.assertEqual((root / 'config/LAYERX_HUMAN_ORIGIN').read_text(), 'https://human.beta.layerx.example')
            self.assertEqual((root / 'config/LAYERX_HUMAN_RP_ID').read_text(), 'human.beta.layerx.example')

    def test_bring_up_publishes_the_web_origin_the_ceremony_configuration_uses(self):
        cluster = (ROOT / 'platform/hosted/tests/beta-cluster.sh').read_text()
        material_source = (ROOT / 'platform/hosted/human/material.sh').read_text()
        self.assertIn('HUMAN_WEB_HOST=human.testnet.layerx.network\n', cluster)
        self.assertIn('HUMAN_WEB_URL="https://$HUMAN_WEB_HOST"\n', cluster)
        self.assertIn('"$LAYERX_BETA_HUMAN_POLICY_FILE" "${HUMAN_WEB_URL:-}"\n', material_source)
        node = yaml.safe_load_all((ROOT / 'platform/hosted/node/deployment.yaml').read_text())
        human = next(c for d in node if d['kind'] == 'StatefulSet'
                     for c in d['spec']['template']['spec']['containers'] if c['name'] == 'human')
        origin = next(e['value'] for e in human['env'] if e['name'] == 'LAYERX_HUMAN_WEB_ORIGIN')
        self.assertEqual(origin, 'https://human.testnet.layerx.network')
        web = next(c for d in yaml.safe_load_all((ROOT / 'platform/hosted/human/web-deployment.yaml').read_text())
                   if d['kind'] == 'Deployment'
                   for c in d['spec']['template']['spec']['containers'] if c['name'] == 'web')
        self.assertEqual(next(e['value'] for e in web['env'] if e['name'] == 'LAYERX_HUMAN_WEB_ORIGIN'), origin)
        self.assertEqual(material.passkey_relying_party(origin)[1], origin)

    def test_explorer_read_principal_is_generated_admitted_and_published(self):
        cluster = (ROOT / 'platform/hosted/tests/beta-cluster.sh').read_text()
        provision = (ROOT / 'platform/hosted/human/provision.sh').read_text()
        self.assertEqual(cluster.count('    explorer_read_principal_generate "$d"\n'), 1)
        self.assertIn('openssl genpkey -algorithm ed25519 -out "$d/explorer-read.key"', cluster)
        self.assertNotIn('LAYERX_BETA_EXPLORER_READ', cluster)
        self.assertNotIn('LAYERX_BETA_EXPLORER_READ', provision)
        published = next(line + following for line, following in zip(cluster.splitlines(), cluster.splitlines()[1:])
                         if 'apply_secret "$ns" layerx-explorer-index ' in line)
        self.assertIn('--from-file=program-token="$s/explorer-program.token"', published)
        self.assertIn('--from-file=read-key="$s/explorer-read.seed.hex"', published)
        self.assertIn('--from-file=sequencer-public-key="$s/sequencer-public-key"', published)
        self.assertIn('explorer_did="did:layerx:$explorer_public"\n', provision)
        self.assertIn('cat "$input/owner-admission.txt" > "$input/genesis-admission.txt"\n', provision)
        self.assertIn("    ' < \"$input/genesis-admission.txt\"\n", provision)
        self.assertNotIn("' < \"$input/owner-admission.txt\"", provision)
        self.assertIn('--actor "$explorer_did"', provision)
        self.assertIn('fail "explorer-read.pub.hex: the explorer read principal key is missing from $SECRETS_DIR"', provision)
        registry = list(yaml.safe_load_all((ROOT / 'platform/hosted/registry/deployment.yaml').read_text()))
        pod = next(d for d in registry if d['kind'] == 'StatefulSet')['spec']['template']['spec']
        index = next(c for c in pod['containers'] if c['name'] == 'explorer-index')
        env = {e['name']: e for e in index['env']}
        mount = next(m for m in index['volumeMounts'] if m['name'] == 'explorer-read')
        self.assertTrue(mount['readOnly'])
        volume = next(v for v in pod['volumes'] if v['name'] == 'explorer-read')['secret']
        self.assertEqual(volume['secretName'], 'layerx-explorer-index')
        self.assertNotIn('optional', volume)
        self.assertEqual({item['key'] for item in volume['items']}, {'read-key', 'sequencer-public-key'})
        paths = {mount['mountPath'] + '/' + item['path'] for item in volume['items']}
        self.assertEqual({env['LAYERX_EXPLORER_READ_KEY_FILE']['value'],
                          env['LAYERX_EXPLORER_READ_SEQUENCER_PUBLIC_KEY_FILE']['value']}, paths)
        self.assertEqual(env['LAYERX_EXPLORER_READ_ENDPOINT']['value'],
                         'https://layerx-pending-core.layerx-testnet.svc.cluster.local:9443')
        self.assertEqual(env['LAYERX_EXPLORER_READ_CA_DER']['value'], env['LAYERX_EXPLORER_AUTHORITY_CA_DER']['value'])
        self.assertEqual(env['LAYERX_EXPLORER_READ_NETWORK_ID']['valueFrom']['configMapKeyRef'],
                         {'name': 'layerx-node-config', 'key': 'network-id'})
        self.assertEqual(env['LAYERX_EXPLORER_READ_FEE_LIMIT']['value'], '0')
        self.assertIn('[ -s "$LAYERX_EXPLORER_OBSERVATION_DIR/naming-program" ]', index['args'][0])
        self.assertIn('export LAYERX_EXPLORER_NAMING_PROGRAM\n', index['args'][0])
        bridge = next(d for d in registry if d['kind'] == 'NetworkPolicy' and d['metadata']['name'] == 'layerx-registry-lni-bridge')
        self.assertEqual(bridge['spec']['podSelector'], {'matchLabels': {'app': 'layerx-node'}})
        self.assertIn({'protocol': 'TCP', 'port': 9443}, bridge['spec']['ingress'][0]['ports'])

    def test_bootstrap_has_no_unpublished_human_secret_dependencies(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'node.yaml'
            subprocess.run([sys.executable, str(HERE / 'bootstrap.py'),
                            str(ROOT / 'platform/hosted/node/deployment.yaml'), str(output)], check=True)
            stateful = next(d for d in yaml.safe_load_all(output.read_text()) if d['kind'] == 'StatefulSet')
            pod = stateful['spec']['template']['spec']
            self.assertIn('layerxd', {c['name'] for c in pod['containers']})
            self.assertTrue({'guarantor-1', 'guarantor-2'} <= {c['name'] for c in pod['containers']})
            self.assertFalse(any(c['name'].startswith('human') for c in pod['containers']))
            self.assertFalse(any(v.get('secret', {}).get('secretName', '').startswith('layerx-human-')
                                 for v in pod['volumes']))
            self.assertTrue(any(v.get('persistentVolumeClaim', {}).get('claimName') == 'layerx-human-state'
                                for v in pod['volumes']))

    def test_real_boundary_certificate_names_and_kms_client_issuance(self):
        cluster = (ROOT / 'platform/hosted/tests/beta-cluster.sh').read_text()
        issue = 'issue_cert() {' + cluster.split('issue_cert() {', 1)[1].split('\n}\n', 1)[0] + '\n}\n'
        call = 'issue_cert paxeer-boundary' + cluster.split('issue_cert paxeer-boundary', 1)[1].split('\n    issue_client_identity', 1)[0]
        with tempfile.TemporaryDirectory() as directory:
            env = dict(os.environ, CA_DIR=directory, TESTNET_NAMESPACE='layerx-testnet')
            script = ('set -euo pipefail\nsvc=layerx-testnet.svc.cluster.local\n' + issue
                      + 'openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes '
                      '-keyout "$CA_DIR/ca.key" -out "$CA_DIR/ca.crt" -days 1 -subj /CN=test-ca '
                      '-addext basicConstraints=critical,CA:TRUE >/dev/null 2>&1\n'
                      + call + '\nissue_cert executor executor clientAuth ""\n')
            subprocess.run(['bash', '-c', script], env=env, check=True)
            certificate = Path(directory) / 'paxeer-boundary/cert.pem'
            for host in ('paxeer-boundary.layerx-testnet.svc.cluster.local',
                         'paxeer-observer-boundary.layerx-testnet.svc.cluster.local'):
                subprocess.run(['openssl', 'verify', '-CAfile', str(Path(directory) / 'ca.crt'),
                                '-verify_hostname', host, str(certificate)], check=True, capture_output=True)
            refused = subprocess.run(['openssl', 'verify', '-CAfile', str(Path(directory) / 'ca.crt'),
                                      '-verify_hostname', 'unlisted.layerx-testnet.svc.cluster.local',
                                      str(certificate)], capture_output=True)
            self.assertNotEqual(refused.returncode, 0)
            subprocess.run(['openssl', 'verify', '-CAfile', str(Path(directory) / 'ca.crt'),
                            '-purpose', 'sslclient', str(Path(directory) / 'executor/cert.pem')],
                           check=True, capture_output=True)

    def test_real_observer_topology_refuses_missing_egress(self):
        source = (ROOT / 'platform/hosted/tests/topology-check.sh').read_text()
        code = source.split("<<'PY'\n", 1)[1].split('\nPY\n', 1)[0]
        module = {}
        exec(compile(code.rsplit('sys.exit(main())', 1)[0], 'topology-check.sh', 'exec'), module)
        cluster = (ROOT / 'platform/hosted/tests/beta-cluster.sh').read_text()
        observer = cluster.split("<<'PYOBSERVER'\n", 1)[1].split('\nPYOBSERVER\n', 1)[0]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'paxeer.yaml'
            path.write_text((ROOT / 'platform/hosted/paxeer/deployment.yaml').read_text())
            subprocess.run([sys.executable, '-c', observer, str(path)], check=True)
            topology = module['Topology']()
            for manifest in (ROOT / 'platform/hosted/node/deployment.yaml', path):
                for document in yaml.safe_load_all(manifest.read_text()):
                    topology.add(document, str(manifest), 'default')
            node = next(w for w in topology.workloads if w['name'] == 'layerx-node')
            paxeer = next(w for w in topology.workloads if w['name'] == 'paxeer')
            service = topology.services[('layerx-testnet', 'paxeer-observer-boundary')]
            self.assertEqual(service['ports'][0]['targetPort'], 'observer-https')
            self.assertEqual(paxeer['ports']['observer-https'], ('9444', 'TCP'))
            def admits():
                return topology.egress_admits(node, paxeer['ns'], paxeer['labels'], '9444', 'TCP', paxeer['ports'])[0]
            self.assertTrue(admits())
            self.assertTrue(topology.ingress_admits(paxeer, '9444', 'TCP', node)[0])
            policy = next(p for p in topology.policies if p['name'] == 'layerx-node-egress')
            policy['egress'] = [r for r in policy['egress'] if not any(str(p['port']) == '9444' for p in r.get('ports', []))]
            self.assertFalse(admits())
            self.assertTrue(topology.egress_admits(node, paxeer['ns'], paxeer['labels'], '9443', 'TCP', paxeer['ports'])[0])


if __name__ == '__main__':
    unittest.main()
