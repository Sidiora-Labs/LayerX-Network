import datetime
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.x509.oid import NameOID


def run(native, directory, executable, socket_path, output):
    public = dict(line.split('=', 1) for line in (native / 'data/node.env').read_text().splitlines())
    material = directory / 'programs'
    material.mkdir(mode=0o700)
    os.chown(material, 4021, 4021)

    def write(name, value):
        path = material / name
        path.write_bytes(value)
        path.chmod(0o600)
        os.chown(path, 4021, 4021)
        return str(path)

    key = ec.generate_private_key(ec.SECP256R1())
    name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, 'native-programs-handover')])
    now = datetime.datetime.now(datetime.timezone.utc)
    ca = (x509.CertificateBuilder().subject_name(name).issuer_name(name)
          .public_key(key.public_key()).serial_number(x509.random_serial_number())
          .not_valid_before(now - datetime.timedelta(minutes=1))
          .not_valid_after(now + datetime.timedelta(days=1))
          .add_extension(x509.BasicConstraints(ca=True, path_length=0), critical=True)
          .sign(key, hashes.SHA256()))
    trust = (b'LayerX/sequencer-trust-history/v1\0' + struct.pack('>HH', 1, 0)
             + struct.pack('>HIQ', 3, 77, 1)
             + bytes.fromhex(public['LAYERX_NODE_SEQUENCER_ID'])
             + bytes.fromhex(public['LAYERX_NODE_SEQUENCER_PUBLIC_KEY'])
             + struct.pack('>QQBQ', 1, 2**64 - 1, 0, 0))
    wasm = Path(os.environ['LAYERX_TEST_HANDOVER_ESCROW_WASM'])
    assert wasm.is_file() and not wasm.is_symlink()
    config = dict(endpoint=public['LAYERX_NODE_PROGRAM_URL'],
        replica_endpoint=public['LAYERX_NODE_REPLICA_URL'], replica_id=public['LAYERX_NODE_REPLICA_ID'],
        token_file=write('node.token', Path(public['LAYERX_NODE_PROGRAM_BEARER_TOKEN_FILE']).read_bytes()),
        replica_token_file=write('replica.token', Path(public['LAYERX_NODE_REPLICA_BEARER_TOKEN_FILE']).read_bytes()),
        ca_file=write('ca.der', ca.public_bytes(serialization.Encoding.DER)),
        trust_file=write('trust.bin', trust), wasm_file=write('escrow.wasm', wasm.read_bytes()))
    config_path = write('config.json', json.dumps(config).encode())
    binary = material / 'native-handover-programs'
    shutil.copyfile(executable, binary)
    binary.chmod(0o755)
    clock = directory / 'runtime-clock'
    with output.open('wb') as log:
        subprocess.run(['setpriv', '--reuid=4021', '--regid=4021', '--clear-groups',
            str(clock), '--runtime-dir', str(directory), '--', str(binary), str(socket_path),
            str(directory), config_path], stdout=log, stderr=log, timeout=180, check=True)
