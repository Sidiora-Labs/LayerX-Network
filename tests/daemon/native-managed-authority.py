import datetime
import json
import os
from pathlib import Path
import shutil
import socket
import ssl
import subprocess
import time
import urllib.request

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.x509.oid import ExtendedKeyUsageOID, NameOID

ROOT = Path(__file__).resolve().parents[2]


def protected(path, content):
    pending = path.with_suffix(path.suffix + '.pending')
    with pending.open('xb') as output:
        output.write(content)
        output.flush()
        os.fsync(output.fileno())
    os.chown(pending, 4021, 4021)
    pending.chmod(0o600)
    os.replace(pending, path)
    return path


class Authority:
    def __init__(self, native, control, build, environment):
        self.root = control / 'managed-authority'
        self.root.mkdir(mode=0o700)
        os.chown(self.root, 4021, 4021)
        self.state = self.root / 'state'
        self.state.mkdir(mode=0o700)
        os.chown(self.state, 4021, 4021)
        self.native = native
        public = dict(line.split('=', 1) for line in (native / 'data/node.env').read_text().splitlines())
        now = datetime.datetime.now(datetime.timezone.utc)
        key = ec.generate_private_key(ec.SECP256R1())
        name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, 'managed-limit-authority')])
        ca = (x509.CertificateBuilder().subject_name(name).issuer_name(name).public_key(key.public_key())
              .serial_number(x509.random_serial_number()).not_valid_before(now - datetime.timedelta(minutes=1))
              .not_valid_after(now + datetime.timedelta(days=1))
              .add_extension(x509.BasicConstraints(ca=True, path_length=0), critical=True)
              .sign(key, hashes.SHA256()))
        server_key = ec.generate_private_key(ec.SECP256R1())
        server_name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, 'localhost')])
        server = (x509.CertificateBuilder().subject_name(server_name).issuer_name(name)
                  .public_key(server_key.public_key()).serial_number(x509.random_serial_number())
                  .not_valid_before(now - datetime.timedelta(minutes=1)).not_valid_after(now + datetime.timedelta(days=1))
                  .add_extension(x509.BasicConstraints(ca=False, path_length=None), critical=True)
                  .add_extension(x509.SubjectAlternativeName([x509.DNSName('localhost')]), critical=False)
                  .add_extension(x509.ExtendedKeyUsage([ExtendedKeyUsageOID.SERVER_AUTH]), critical=False)
                  .sign(key, hashes.SHA256()))
        certificate = protected(self.root / 'server.der', server.public_bytes(serialization.Encoding.DER))
        private = protected(self.root / 'server-key.der', server_key.private_bytes(serialization.Encoding.DER,
                            serialization.PrivateFormat.PKCS8, serialization.NoEncryption()))
        self.ca = protected(self.root / 'ca.der', ca.public_bytes(serialization.Encoding.DER))
        pem = protected(self.root / 'ca.pem', ca.public_bytes(serialization.Encoding.PEM))
        self.token = protected(self.root / 'authority.token', os.urandom(32).hex().encode())
        self.human_token = protected(self.root / 'human.token', os.urandom(32).hex().encode())
        replica_token = protected(self.root / 'replica.token', Path(public['LAYERX_NODE_REPLICA_BEARER_TOKEN_FILE']).read_bytes())
        binary = self.root / 'layerx-receipt-authority'
        shutil.copyfile(environment['LAYERX_TEST_MANAGED_AUTHORITY_BIN'], binary)
        binary.chmod(0o755)
        self.registry = self.root / 'module-registry.json'
        metadata = dict(line.split('=', 1) for line in (ROOT / 'platform/hosted/node/bootstrap.sh').read_text().splitlines()
                        if line.startswith(('ASSET_SYMBOL=', 'ASSET_CURRENCY=', 'ASSET_DECIMALS=')))
        with self.registry.open('xb') as output:
            subprocess.run([str(build / 'bin/layerx-module-registry'), 'generate', '--network-id', '77',
                '--protocol-version', '3', '--asset', public['LAYERX_NODE_ASSET_ID'],
                '--symbol', metadata['ASSET_SYMBOL'], '--currency', metadata['ASSET_CURRENCY'],
                '--decimals', metadata['ASSET_DECIMALS'], '--custody-profile', environment['LAYERX_TEST_WITHDRAW_PROFILE']],
                stdout=output, check=True, timeout=15)
        os.chown(self.registry, 4021, 4021)
        self.registry.chmod(0o600)
        with socket.socket() as reserve:
            reserve.bind(('127.0.0.1', 0))
            port = reserve.getsockname()[1]
        self.url = f'https://localhost:{port}'
        self.context = ssl.create_default_context(cafile=str(pem))
        authority = (control / 'authority.csv').read_text().splitlines()
        assert authority[0] == 'layerx-sequencer-authority-v1' and len(authority) == 2
        identity, sequencer, _, first, last, active = authority[1].split(',')
        assert identity == public['LAYERX_NODE_SEQUENCER_ID'] and sequencer == public['LAYERX_NODE_SEQUENCER_PUBLIC_KEY'] and active == 'active'
        self.environment = {k: v for k, v in environment.items() if not k.startswith('LAYERX_AUTHORITY_')}
        self.environment.update(LAYERX_AUTHORITY_LISTEN=f'127.0.0.1:{port}',
            LAYERX_AUTHORITY_TLS_CERT_DER=str(certificate), LAYERX_AUTHORITY_TLS_KEY_DER=str(private),
            LAYERX_AUTHORITY_TOKEN_FILES=str(self.token), LAYERX_AUTHORITY_REPLICA_URL=public['LAYERX_NODE_REPLICA_URL'],
            LAYERX_AUTHORITY_REPLICA_BEARER_TOKEN_FILE=str(replica_token), LAYERX_AUTHORITY_REPLICA_ID=public['LAYERX_NODE_REPLICA_ID'],
            LAYERX_AUTHORITY_LNI_SOCKET=public['LAYERX_NODE_LNI_SOCKET'],
            LAYERX_AUTHORITY_PROTOCOL_NETWORK_ID='77', LAYERX_AUTHORITY_NETWORK_ID='native-managed-limit',
            LAYERX_AUTHORITY_WIRE_VERSION='3', LAYERX_AUTHORITY_SEQUENCER_ID=public['LAYERX_NODE_SEQUENCER_ID'],
            LAYERX_AUTHORITY_SEQUENCER_PUBLIC_KEY=public['LAYERX_NODE_SEQUENCER_PUBLIC_KEY'],
            LAYERX_AUTHORITY_FIRST_BATCH=first,
            LAYERX_AUTHORITY_LAST_BATCH=last)
        self.command = ['setpriv', '--reuid=4021', '--regid=4021', '--clear-groups', str(binary)]
        self.process = None
        self.log = None
        self.generation = 0
        self.start()

    def query(self, route):
        request = urllib.request.Request(self.url + route,
            headers={'Authorization': 'Bearer ' + self.token.read_text()})
        with urllib.request.urlopen(request, context=self.context, timeout=15) as response:
            return json.load(response)

    def start(self):
        self.log = (self.root / f'service-{self.generation}.log').open('xb')
        self.process = subprocess.Popen(self.command, env=self.environment, stdout=self.log, stderr=self.log)
        self.generation += 1
        deadline = time.monotonic() + 15
        while True:
            assert self.process.poll() is None, 'managed Authority exited before readiness'
            try:
                self.query('/readyz')
                return
            except OSError:
                assert time.monotonic() < deadline, 'managed Authority readiness deadline'
                time.sleep(.05)

    def close(self):
        if self.process is not None:
            if self.process.poll() is None:
                self.process.terminate()
                assert self.process.wait(timeout=15) == 0, 'managed Authority shutdown refused'
            self.process = None
        if self.log is not None:
            self.log.close()
            self.log = None

    def configure(self, request):
        assert set(request) == {'version', 'policy', 'binding', 'clock', 'receipts'} and request['version'] == 1
        records = request['receipts']
        assert isinstance(records, list) and 5 <= len(records) <= 16
        identifiers = []
        for record in records:
            assert set(record) == {'activity_id', 'receipt_digest'}
            activity, digest = record['activity_id'], record['receipt_digest']
            assert all(isinstance(value, str) and len(value) == 64 and bytes.fromhex(value).hex() == value
                       for value in (activity, digest))
            assert activity not in identifiers
            authorized = self.query('/v1/authorized-batches/by-activity/' + activity)
            assert authorized['activity_id'] == activity
            receipt = bytes.fromhex(authorized['receipt'])
            assert 0 < len(receipt) <= 1048576
            batch = authorized['batch_id']
            document = self.query(f'/v1/batches/{batch}/receipt-authority?receipt_digest={digest}')
            protected(self.state / (activity + '.json'), json.dumps(dict(receipt_hex=receipt.hex(),
                replica_document=document), sort_keys=True, separators=(',', ':')).encode())
            identifiers.append(activity)
        policy = request['policy']
        assert len(policy['principals']) == 1
        principal = policy['principals'][0]
        assert principal['tenant'] == 'native-managed-limit' and principal['principal'] == 'owner'
        principal['activities'] = sorted(identifiers)
        policy_path = protected(self.root / 'policy.json', json.dumps(policy, sort_keys=True).encode())
        if 'LAYERX_AUTHORITY_HUMAN_AGENT_TOKEN_FILE' not in self.environment:
            self.close()
            assert set(request['clock']) == {'LAYERX_RUNTIME_CLOCK_SOCKET', 'LAYERX_RUNTIME_CLOCK_PID', 'LAYERX_RUNTIME_CLOCK_UID'}
            assert request['clock']['LAYERX_RUNTIME_CLOCK_UID'] == '4021'
            self.environment.update(request['clock'])
            self.environment.update(LAYERX_AUTHORITY_IDENTITY_BINDING_SOCKET=request['binding'],
                LAYERX_AUTHORITY_IDENTITY_BINDING_UID='4021', LAYERX_AUTHORITY_IDENTITY_BINDING_GID='4021')
            self.environment.update(LAYERX_AUTHORITY_HUMAN_AGENT_TOKEN_FILE=str(self.human_token),
                LAYERX_AUTHORITY_HUMAN_AGENT_TENANT='native-managed-limit', LAYERX_AUTHORITY_HUMAN_AGENT_PRINCIPAL='owner',
                LAYERX_AUTHORITY_PRINCIPAL_POLICY_FILE=str(policy_path), LAYERX_AUTHORITY_MODULE_REGISTRY_FILE=str(self.registry),
                LAYERX_AUTHORITY_CORE_CLOCK_HORIZON='10000', LAYERX_AUTHORITY_STATE_ROOT=str(self.state))
            self.start()
        return {'version': 1, 'endpoint': self.url, 'token': str(self.human_token), 'ca': str(self.ca)}
