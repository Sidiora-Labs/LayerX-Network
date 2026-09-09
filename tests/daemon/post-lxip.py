import json
import os
from pathlib import Path
import struct
import shutil
import subprocess
import sys
import time

repo = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(repo / 'platform/hosted/human'))
from guardians import generate
from owner_native import digest, prepare_admission, protected_write, receipt, receipt_fields, span
from provision import write_json
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

work = Path(sys.argv[1])
provider = repo / 'human/target/debug/layerx-human-identity-provider'
cli = repo / 'cmd/layerxctl/target/debug/layerxctl'
socket = Path(os.environ.get('LAYERX_TEST_OWNER_AUTHORITY_SOCKET',
                             work / 'run/layerxd.lni.sock'))
shutil.copyfile(cli, work / 'native-cli')
cli = work / 'native-cli'
cli.chmod(0o755)
generate(work, work, [digest(b'operator', bytes([i])).hex() for i in range(3)])
root = work / 'human-evidence-input'
request = dict(email='owner@example.com', display_name='Owner', idempotency_key='native-admission', now=1)
env = dict(os.environ, LAYERX_HUMAN_IDENTITY_PROVIDER_STATE_ROOT=str(work / 'lxip'),
           LAYERX_HUMAN_IDENTITY_PROVIDER_RECOVERY_POLICY_FILE=str(root / 'recovery-policy.json'))
owner = json.loads(subprocess.run([str(provider), 'provision-owner'], input=json.dumps(request).encode(),
                                 capture_output=True, env=env, check=True).stdout)
write_json(work / 'human-owner-result.json', owner)
prepare_admission(work, work)
binding = json.loads((root / 'owner-admission.json').read_text())
assert binding['did'] == owner['did']
key = Ed25519PrivateKey.from_private_bytes((work / 'human-owner/owner.seed').read_bytes())
public = key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
assert public.hex() == binding['public_key']
common = ['--socket', str(socket), '--network-id', '77', '--protocol-version', '3', '--actor', owner['did']]

def command(*args):
    return subprocess.run(['setpriv', '--reuid=4021', '--regid=4021', '--clear-groups', str(cli), *args, *common], capture_output=True)

assert command('read-state').returncode != 0
identity_file = work / 'data/identities.txt'
original = identity_file.read_bytes()
protected_write(identity_file.with_suffix('.pending'), original + (root / 'owner-admission.txt').read_bytes())
os.replace(identity_file.with_suffix('.pending'), identity_file)
for _ in range(100):
    state = command('read-state')
    if state.returncode == 0:
        break
    time.sleep(0.1)
assert state.returncode == 0, 'post-LXIP native preparation refused'
state = json.loads(state.stdout)
assert state['account_sequence'] == 0 and state['global_sequence'] == 0
if '--prepare-only' in sys.argv:
    print('real LXIP owner and protected post-bootstrap native admission prepared')
    sys.exit(0)
actor = owner['did'].encode()
now = time.time_ns() // 1000000
payload = b'\x71\1\0\2' + digest(b'did-id', struct.pack('>H', len(actor)) + actor) + public
body = (b'\1\0\3\2' + struct.pack('>I', 77) + b'\3' + struct.pack('>I', 0x70001)
        + b'\4' + span(actor) + b'\5' + span(public) + b'\6' + struct.pack('>Q', 0)
        + b'\7' + struct.pack('>QQ', now, now + 300000) + b'\10' + span(os.urandom(32))
        + b'\11' + bytes(16) + b'\12' + span(digest(b'payload-hash', payload)) + b'\13' + span(payload))
signed = b'\0\3\x10\1\14' + body + b'\14' + span(key.sign(digest(b'signature-preimage', b'\0\3\x10\1\13' + body)))
activity = work / 'owner.activity'
activity.write_bytes(signed)
activity.chmod(0o644)
result = command('submit', '--public-key', public.hex(), '--activity', str(activity))
assert result.returncode == 0, 'post-LXIP signed admission failed'
activity_id = digest(b'activity-id', signed)
assert json.loads(result.stdout)['activity_id'] == activity_id.hex()
# The LNI authenticates the client's uid, including receipt queries.
raw_path = work / 'owner.receipt'
raw_path.touch(mode=0o600)
os.chown(raw_path, 4021, 4021)
child = os.fork()
if child == 0:
    os.setgroups([])
    os.setgid(4021)
    os.setuid(4021)
    raw_path.write_bytes(receipt(str(socket), activity_id))
    os._exit(0)
assert os.waitpid(child, 0)[1] == 0
sequencer = Ed25519PrivateKey.from_private_bytes(bytes([0x22]) * 32).public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
verified = receipt_fields(raw_path.read_bytes(), sequencer, raw_path)
assert verified['module'] == 7 and verified['sequence'] == 1
assert verified['effects'][0][4][5:37] == digest(b'did-id', struct.pack('>H', len(actor)) + actor)
assert command('submit', '--public-key', public.hex(), '--activity', str(activity)).returncode == 0
assert json.loads(command('read-state').stdout)['account_sequence'] == 1
print('real LXIP DID, post-bootstrap native admission, signed Governance receipt and idempotent retry passed')
