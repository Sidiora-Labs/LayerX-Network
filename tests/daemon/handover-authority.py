import datetime
import json
import os
from pathlib import Path
import shutil
import socket
import ssl
import subprocess
import tempfile
import time
import urllib.error
import urllib.request

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.x509.oid import ExtendedKeyUsageOID, NameOID


def records(path):
    data = path.read_bytes()
    cursor, result = 0, []
    while cursor + 32 <= len(data) and data[cursor:cursor + 4] != bytes(4):
        assert data[cursor:cursor + 4] == b'LXPL'
        size = int.from_bytes(data[cursor + 16:cursor + 20], 'big')
        body = data[cursor + 32:cursor + 32 + size]
        assert len(body) == size and body[:4] == b'LXBE'
        offset = 79 + int.from_bytes(body[77:79], 'big') + 64
        assert offset + 9 <= len(body)
        depth = body[offset]
        offset += 9 + depth * 32
        length = int.from_bytes(body[offset:offset + 4], 'big')
        receipt = body[offset + 4:offset + 4 + length]
        assert len(receipt) == length
        if body[4] != ord('3'):
            assert receipt[:6] == bytes.fromhex('000352010003')
            assert int.from_bytes(receipt[6:10], 'big') == 32
            result.append((receipt[10:42].hex(), receipt))
        cursor += 32 + size
    assert len(result) >= 2
    return result


def run(native, directory, executable, socket_path):
    public = dict(line.split('=', 1) for line in (native / 'data/node.env').read_text().splitlines())
    target = Path(tempfile.mkdtemp(prefix='handover-authority-', dir=native))
    material = target / 'material'
    material.mkdir(mode=0o700)
    os.chown(target, 4021, 4021)
    os.chown(material, 4021, 4021)

    def write(name, content):
        path = material / name
        path.write_bytes(content)
        os.chown(path, 4021, 4021)
        path.chmod(0o600)
        return path

    authority_key = ec.generate_private_key(ec.SECP256R1())
    now = datetime.datetime.now(datetime.timezone.utc)
    name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, 'handover-authority')])
    ca = (x509.CertificateBuilder().subject_name(name).issuer_name(name)
          .public_key(authority_key.public_key()).serial_number(x509.random_serial_number())
          .not_valid_before(now - datetime.timedelta(minutes=1)).not_valid_after(now + datetime.timedelta(days=1))
          .add_extension(x509.BasicConstraints(ca=True, path_length=0), critical=True)
          .sign(authority_key, hashes.SHA256()))
    server_key = ec.generate_private_key(ec.SECP256R1())
    server_name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, 'localhost')])
    certificate = (x509.CertificateBuilder().subject_name(server_name).issuer_name(name)
                   .public_key(server_key.public_key()).serial_number(x509.random_serial_number())
                   .not_valid_before(now - datetime.timedelta(minutes=1)).not_valid_after(now + datetime.timedelta(days=1))
                   .add_extension(x509.BasicConstraints(ca=False, path_length=None), critical=True)
                   .add_extension(x509.SubjectAlternativeName([x509.DNSName('localhost')]), critical=False)
                   .add_extension(x509.ExtendedKeyUsage([ExtendedKeyUsageOID.SERVER_AUTH]), critical=False)
                   .sign(authority_key, hashes.SHA256()))
    certificate_path = write('server.der', certificate.public_bytes(serialization.Encoding.DER))
    key_path = write('server-key.der', server_key.private_bytes(serialization.Encoding.DER, serialization.PrivateFormat.PKCS8, serialization.NoEncryption()))
    ca_path = write('ca.pem', ca.public_bytes(serialization.Encoding.PEM))
    token_path = write('authority.token', os.urandom(32).hex().encode())
    replica_token = write('replica.token', Path(public['LAYERX_NODE_REPLICA_BEARER_TOKEN_FILE']).read_bytes())
    genesis = write('genesis.lxt', (directory / 'handover-genesis.bin').read_bytes())
    finality = write('finality.conf', (directory / 'handover-finality.conf').read_bytes())
    binary = material / 'layerx-receipt-authority'
    shutil.copyfile(executable, binary)
    binary.chmod(0o755)
    with socket.socket() as reserved:
        reserved.bind(('127.0.0.1', 0))
        port = reserved.getsockname()[1]
    environment = {key: value for key, value in os.environ.items() if not key.startswith('LAYERX_AUTHORITY_')}
    environment.update(LAYERX_AUTHORITY_LISTEN=f'127.0.0.1:{port}',
        LAYERX_AUTHORITY_TLS_CERT_DER=str(certificate_path), LAYERX_AUTHORITY_TLS_KEY_DER=str(key_path),
        LAYERX_AUTHORITY_TOKEN_FILES=str(token_path), LAYERX_AUTHORITY_REPLICA_URL=public['LAYERX_NODE_REPLICA_URL'],
        LAYERX_AUTHORITY_REPLICA_BEARER_TOKEN_FILE=str(replica_token), LAYERX_AUTHORITY_REPLICA_ID=public['LAYERX_NODE_REPLICA_ID'],
        LAYERX_AUTHORITY_LNI_SOCKET=str(socket_path), LAYERX_AUTHORITY_PROTOCOL_NETWORK_ID=public['LAYERX_NODE_NETWORK_ID'],
        LAYERX_AUTHORITY_NETWORK_ID='handover-authority', LAYERX_AUTHORITY_WIRE_VERSION='3',
        LAYERX_AUTHORITY_SEQUENCER_ID=public['LAYERX_NODE_SEQUENCER_ID'],
        LAYERX_AUTHORITY_SEQUENCER_PUBLIC_KEY=public['LAYERX_NODE_SEQUENCER_PUBLIC_KEY'],
        LAYERX_AUTHORITY_FIRST_BATCH='1', LAYERX_AUTHORITY_LAST_BATCH=str(2**64 - 1),
        LAYERX_AUTHORITY_GENESIS_TRUST=str(genesis), LAYERX_AUTHORITY_HANDOVER_FINALITY=str(finality))
    command = ['setpriv', '--reuid=4021', '--regid=4021', '--clear-groups', str(binary)]
    context = ssl.create_default_context(cafile=str(ca_path))
    token = token_path.read_text()

    def request(path, bearer=token):
        req = urllib.request.Request(f'https://localhost:{port}{path}', headers={'Authorization': 'Bearer ' + bearer})
        try:
            with urllib.request.urlopen(req, context=context, timeout=10) as response:
                return response.status, json.load(response)
        except urllib.error.HTTPError as error:
            return error.code, json.load(error)

    expected = records(native / 'data/logs/receipt-authority.log')
    observed = []
    for restart in range(2):
        with (target / f'service-{restart}.log').open('wb') as log:
            process = subprocess.Popen(command, env=environment, stdout=log, stderr=log)
            try:
                deadline = time.monotonic() + 8
                while True:
                    assert process.poll() is None, 'Authority exited before readiness'
                    try:
                        if request('/readyz')[0] == 200:
                            break
                    except OSError:
                        pass
                    assert time.monotonic() < deadline, 'Authority readiness deadline'
                    time.sleep(.025)
                answers = []
                for activity, receipt in expected:
                    status, answer = request('/v1/authorized-batches/by-activity/' + activity)
                    assert status == 200, ('verified historical receipt refused', status, answer.get('error'))
                    assert answer['activity_id'] == activity and bytes.fromhex(answer['receipt']) == receipt
                    answers.append(answer)
                assert answers[0]['sequencer_public_key'] == public['LAYERX_NODE_SEQUENCER_PUBLIC_KEY']
                assert answers[-1]['sequencer_public_key'] != answers[0]['sequencer_public_key']
                assert request('/v1/authorized-batches/by-activity/' + 'aa' * 32)[0] == 404
                assert request('/v1/authorized-batches/by-activity/' + expected[0][0], 'wrong-token')[0] == 401
                if observed:
                    assert answers == observed
                observed = answers
            finally:
                process.terminate()
                assert process.wait(timeout=10) == 0
    missing = environment.copy()
    del missing['LAYERX_AUTHORITY_HANDOVER_FINALITY']
    with (target / 'missing-policy.log').open('wb') as log:
        assert subprocess.run(command, env=missing, stdout=log, stderr=log, timeout=8).returncode == 2
    genesis.chmod(0o644)
    with (target / 'unprotected-genesis.log').open('wb') as log:
        assert subprocess.run(command, env=environment, stdout=log, stderr=log, timeout=8).returncode == 2
    genesis.chmod(0o600)
    print('real TLS Authority verified original old and new epoch receipts twice across process restart; unknown activity, bearer, absent policy and unprotected genesis refused')
