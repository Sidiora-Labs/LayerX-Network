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

    def test_custody_bindings_resolve_to_the_native_custody_precompile(self):
        precompile = '0x0000000000000000000000000000000000001013'
        self.assertEqual(material.CUSTODY_PRECOMPILE, precompile)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            evidence = root / 'human-evidence'
            evidence.mkdir(mode=0o700)
            for name, value in (
                    ('components.json', {'AGENT_ACTOR': 'did:layerx:' + '01' * 32}),
                    ('agent.json', {'HUMAN_PEERS': 'uid=4020;tenant=beta;principal=did:layerx:' + '01' * 32}),
                    ('purpose-catalog.json', {'purposes': []}),
                    ('authority.json', {'tenant': 'beta', 'principal': 'did:layerx:' + '01' * 32,
                                        'core-clock-horizon': 60}),
                    ('principal-policy.json', {'principals': []}),
                    ('recovery-policy.json', {'root': list(range(1, 33)), 'threshold': 2,
                                              'delay_seconds': 86400}),
                    ('movement-policy.json', {'CUSTODY_REFERENCE': '0x' + 'ab' * 32,
                                              'PAXEER_CHECKPOINT_AUTHORITY': '0x' + 'cd' * 32,
                                              'PAXEER_CONFIRMATIONS': 12,
                                              'CHECKPOINT_INTERVAL_SECONDS': 60,
                                              'PAXEER_BLOCK_SECONDS': 2,
                                              'REMINDER_INTERVAL_SECONDS': 300})):
                material.write(evidence, name, json.dumps(value))
            registry = root / 'module-registry.json'
            registry.write_text(json.dumps({'schema_version': 2, 'assets': [{'asset': 'a' * 64}],
                                            'modules': [{'module': 8, 'ordinals': [1, 2]}]}))
            deployment = root / 'deployment.json'
            deployed = {'vault': '0x' + '11' * 20, 'withdrawal_claims': '0x' + '22' * 20,
                        'emergency_exit': '0x' + '33' * 20, 'checkpoint_registry': '0x' + '44' * 20}
            deployment.write_text(json.dumps({'network_id': 402, 'chain_id': 125, 'addresses': deployed}))
            output = root / 'policy.json'
            material.assemble_policy(evidence, deployment, registry, output, 402, 125)
            policy = json.loads(output.read_text())
            for name in ('PAXEER_EXIT_CONTRACT', 'PAXEER_WITHDRAWAL_CLAIMS_CONTRACT'):
                self.assertEqual(policy['components'][name], precompile)
            for name in ('PAXEER_VAULT', 'PAXEER_CLAIMS_CONTRACT', 'PAXEER_EXIT_CONTRACT'):
                self.assertEqual(policy['movement'][name], precompile)
            self.assertEqual(policy['movement']['PAXEER_CHECKPOINT_REGISTRY'], deployed['checkpoint_registry'])
            self.assertEqual(policy['movement']['CUSTODY_REFERENCE'], '0x' + 'ab' * 32)
            written = output.read_text()
            for name in ('vault', 'withdrawal_claims', 'emergency_exit'):
                self.assertNotIn(deployed[name], written)

    def test_passkey_relying_party_follows_the_deployed_web_origin(self):
        self.assertEqual(material.passkey_relying_party(''),
                         ('human.layerx.network', 'https://human.layerx.network'))
        self.assertEqual(material.passkey_relying_party('https://human.beta.layerx.example'),
                         ('human.beta.layerx.example', 'https://human.beta.layerx.example'))
        for refused in ('http://human.layerx.network', 'https://localhost:19457',
                        'https://127.0.0.1', 'https://human.layerx.network/',
                        'https://Human.Testnet.Layerx.Network', 'https://human.layerx.network?a=1'):
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
        self.assertIn('HUMAN_WEB_HOST=human.layerx.network\n', cluster)
        self.assertIn('HUMAN_WEB_URL="https://$HUMAN_WEB_HOST"\n', cluster)
        self.assertIn('"$LAYERX_BETA_HUMAN_POLICY_FILE" "${HUMAN_WEB_URL:-}"\n', material_source)
        node = yaml.safe_load_all((ROOT / 'platform/hosted/node/deployment.yaml').read_text())
        human = next(c for d in node if d['kind'] == 'StatefulSet'
                     for c in d['spec']['template']['spec']['containers'] if c['name'] == 'human')
        origin = next(e['value'] for e in human['env'] if e['name'] == 'LAYERX_HUMAN_WEB_ORIGIN')
        self.assertEqual(origin, 'https://human.layerx.network')
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
        self.assertEqual(env['LAYERX_EXPLORER_READ_FEE_LIMIT']['value'], '50000000')
        self.assertIn('[ -s "$LAYERX_EXPLORER_OBSERVATION_DIR/naming-program" ]', index['args'][0])
        self.assertIn('export LAYERX_EXPLORER_NAMING_PROGRAM\n', index['args'][0])
        bridge = next(d for d in registry if d['kind'] == 'NetworkPolicy' and d['metadata']['name'] == 'layerx-registry-lni-bridge')
        self.assertEqual(bridge['spec']['podSelector'], {'matchLabels': {'app': 'layerx-node'}})
        self.assertIn({'protocol': 'TCP', 'port': 9443}, bridge['spec']['ingress'][0]['ports'])

    def test_explorer_read_principal_is_funded_to_cover_its_signed_fee_limit(self):
        import re
        provision = (ROOT / 'platform/hosted/human/provision.sh').read_text()
        reads = (ROOT / 'human/crates/layerx-explorer-index/src/reads.rs').read_text()
        bootstrap = (ROOT / 'platform/hosted/node/bootstrap.sh').read_text()
        resources = [int(value.replace('_', '')) for value in re.search(
            r'const READ_RESOURCES: \[u64; 7\] = \[([0-9_,\s]+)\];', reads).group(1).split(',') if value.strip()]
        prices = [int(value) for value in re.search(r'for price in ((?:[0-9]+ ?){7}); do', bootstrap).group(1).split()]
        self.assertEqual((len(resources), len(prices)), (7, 7))
        ceiling = sum(resource * price for resource, price in zip(resources[:6], prices[:6]))
        self.assertEqual(ceiling, 25117312)
        registry = list(yaml.safe_load_all((ROOT / 'platform/hosted/registry/deployment.yaml').read_text()))
        pod = next(d for d in registry if d['kind'] == 'StatefulSet')['spec']['template']['spec']
        index = next(c for c in pod['containers'] if c['name'] == 'explorer-index')
        fee_limit = int(next(e['value'] for e in index['env'] if e['name'] == 'LAYERX_EXPLORER_READ_FEE_LIMIT'))
        units = int(re.search(r'^EXPLORER_READ_FUNDING_UNITS=([0-9]+)$', provision, re.M).group(1))
        self.assertGreaterEqual(fee_limit, ceiling)
        self.assertGreaterEqual(units, fee_limit)
        self.assertLessEqual(units, 1000000000)
        self.assertNotIn('LAYERX_BETA_EXPLORER', provision)
        body = provision.split('human_evidence_provision() (', 1)[1].split('\n)\n', 1)[0]
        self.assertGreater(body.index('    explorer_read_principal_fund\n'), body.index('    human_native_provision\n'))
        fund = provision.split('explorer_read_principal_fund() (', 1)[1].split('\n)\n', 1)[0]
        steps = ['--prepare-explorer-read-funding', 'human_custody_step deposit "$funding" --amount "$EXPLORER_READ_FUNDING_UNITS"',
                 'layerxctl read-state', '--sign-explorer-read-credit', '-c owner-producer -- sh -ec',
                 '--submit-explorer-read-credit', 'credit-result.json" "$EXPLORER_READ_FUNDING_UNITS"']
        positions = [fund.index(step) for step in steps]
        self.assertEqual(positions, sorted(positions))
        self.assertIn('[ ! -e "$funding" ] || fail', fund)
        self.assertNotIn('seed', fund)
        custody = provision.split('human_custody_step() (', 1)[1].split('\n)\n', 1)[0]
        self.assertIn('local mode=$1 work=${2:-$WORK_DIR}\n', custody)
        self.assertIn('--work-dir "$work" ', custody)
        self.assertIn('human_custody_step deposit\n', provision)

    def test_explorer_read_principal_signs_its_own_custody_credit(self):
        import hashlib
        from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey, Ed25519PublicKey
        from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat
        sys.path.insert(0, str(HERE))
        import owner_native
        import provision
        with tempfile.TemporaryDirectory() as directory:
            work = Path(directory).resolve() / 'work'
            secrets = Path(directory).resolve() / 'secrets'
            source = work / 'human-evidence-input'
            for path in (work, secrets, source):
                path.mkdir(mode=0o700)
            seed = bytes(range(1, 33))
            public = Ed25519PrivateKey.from_private_bytes(seed).public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
            did = b'did:layerx:' + public.hex().encode()
            account = hashlib.sha256(b'LX:ACCOUNT:v1' + owner_native.span(b'agent:' + did + b':main')).digest()
            owner_native.protected_write(secrets / 'explorer-read.seed.hex', seed.hex().encode())
            (secrets / 'explorer-read.pub.hex').write_text(public.hex())
            asset = '11' * 32
            custody = dict(vault='0x' + '0' * 36 + '1013', asset=asset, runtime_sha256='66' * 32, payer='0x' + '77' * 20)
            provision.write_json(source / 'owner-custody.json', custody)
            profile = b'LXBC3' + bytes(92) + bytes.fromhex(asset) + bytes(78)
            owner_native.protected_write(source / 'custody.profile', profile)
            provision.explorer_read_funding(provision._explorer_read_funding_prepare, work, secrets)
            funding = work / 'explorer-read-funding/human-evidence-input'
            self.assertEqual(provision.protected_json(funding / 'owner-admission.json'),
                             dict(did=did.decode(), public_key=public.hex(), owner_account=account.hex()))
            self.assertEqual(provision.protected_json(funding / 'owner-custody.json'), custody)
            self.assertEqual(provision.protected_bytes(funding / 'custody.profile', 223), profile)
            self.assertEqual((work / 'explorer-read-funding').stat().st_mode & 0o777, 0o700)
            with self.assertRaises(provision.Refused):
                provision.explorer_read_funding(provision._explorer_read_funding_prepare, work, secrets)
            deposit = bytes(range(32, 64))
            units = 1000000000
            credit = bytearray(363 + 600)
            credit[:5] = b'LXDC3'
            credit[43:75] = deposit
            credit[75:107] = bytes.fromhex(asset)
            credit[107:139] = account
            credit[139:171] = public
            credit[191:207] = units.to_bytes(16, 'big')
            state = work / 'explorer-read-funding/read-state.json'
            provision.write_json(state, dict(network_id=402, protocol_version=3, global_sequence=9, account_sequence=0))
            foreign = bytearray(credit)
            foreign[107:139] = bytes(32)
            owner_native.protected_write(funding / 'custody-credit.bin', bytes(foreign))
            with self.assertRaises(provision.Refused):
                provision.explorer_read_funding(provision._explorer_read_credit_sign, work, secrets, 402, state,
                                                work / 'explorer-read-funding/credit-request.json')
            (funding / 'custody-credit.bin').unlink()
            owner_native.protected_write(funding / 'custody-credit.bin', bytes(credit))
            output = work / 'explorer-read-funding/credit-request.json'
            provision.explorer_read_funding(provision._explorer_read_credit_sign, work, secrets, 402, state, output)
            request = provision.protected_json(output)
            self.assertEqual({name: request[name] for name in ('did', 'public_key', 'account', 'amount')},
                             dict(did=did.decode(), public_key=public.hex(), account=account.hex(), amount=units))
            signed = bytes.fromhex(request['activity'])
            reader = owner_native.Reader(signed, output)
            self.assertEqual(reader.take(5), b'\0\3\x10\1\14')
            self.assertEqual((reader.take(1), reader.number(2)), (b'\1', 3))
            self.assertEqual((reader.take(1), reader.number(4)), (b'\2', 402))
            self.assertEqual((reader.take(1), reader.number(4)), (b'\3', (8 << 16) | 1))
            self.assertEqual((reader.take(1), reader.span(255)), (b'\4', did))
            self.assertEqual((reader.take(1), reader.span(32)), (b'\5', public))
            self.assertEqual((reader.take(1), reader.number(8)), (b'\6', 0))
            self.assertEqual(reader.take(1), b'\7')
            not_before, expires = reader.number(8), reader.number(8)
            self.assertEqual(expires - not_before, 300000)
            self.assertEqual((reader.take(1), reader.span(32)),
                             (b'\10', hashlib.sha256(b'LX:DEPOSIT:NULLIFIER:v1' + deposit).digest()))
            self.assertEqual((reader.take(1), reader.number(16)), (b'\11', 0))
            self.assertEqual((reader.take(1), reader.span(32)),
                             (b'\12', owner_native.digest(b'payload-hash', bytes(credit))))
            self.assertEqual((reader.take(1), reader.span(len(credit))), (b'\13', bytes(credit)))
            unsigned = b'\0\3\x10\1\13' + signed[5:reader.offset]
            self.assertEqual(reader.take(1), b'\14')
            signature = reader.span(64)
            reader.finish()
            Ed25519PublicKey.from_public_bytes(public).verify(
                signature, owner_native.digest(b'signature-preimage', unsigned))
            self.assertNotIn(seed.hex(), output.read_text())

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
