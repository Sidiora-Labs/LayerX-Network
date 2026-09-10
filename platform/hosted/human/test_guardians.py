import hashlib
import json
from pathlib import Path
import subprocess
import sys

import pytest
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey, Ed25519PublicKey
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

import guardians
import owner_native
from provision import Refused

TOOL = str(Path(guardians.__file__).resolve())
IDENTITIES = {role: hashlib.sha256(role.encode()).hexdigest() for role in owner_native.GUARDIAN_ROLES}


def directories(tmp_path):
    work, secrets = tmp_path / 'work', tmp_path / 'secrets'
    work.mkdir()
    secrets.mkdir()
    return work, secrets


def enrolled_set(work, secrets):
    for role in owner_native.GUARDIAN_ROLES:
        guardians.enroll(work, secrets, role, IDENTITIES[role], 1)
    return guardians.assemble(work)


def public_key(member):
    return bytes.fromhex(member['public_key'])


def test_each_guardian_enrols_in_its_own_process_and_custody_directory(tmp_path):
    work, secrets = directories(tmp_path)
    for role in owner_native.GUARDIAN_ROLES:
        completed = subprocess.run([sys.executable, TOOL, 'enroll', '--work-dir', str(work),
                                    '--secrets-dir', str(secrets), '--role', role, '--identity', IDENTITIES[role]],
                                   capture_output=True, text=True)
        assert completed.returncode == 0, completed.stderr
    completed = subprocess.run([sys.executable, TOOL, 'assemble', '--work-dir', str(work)],
                               capture_output=True, text=True)
    assert completed.returncode == 0, completed.stderr
    root = work / 'human-evidence-input'
    document = json.loads((root / 'recovery-guardians.json').read_text())
    assert document == json.loads((root / 'recovery-guardian-bindings.json').read_text())
    assert document['version'] == 1 and document['threshold'] == 2 and len(document['members']) == 3
    custody = set()
    for member in document['members']:
        directory = Path(member['custody'])
        assert directory.parent == (secrets / 'human-guardians').resolve()
        assert directory.stat().st_mode & 0o777 == 0o700
        assert [entry.name for entry in directory.iterdir()] == ['seed']
        assert (directory / 'seed').stat().st_mode & 0o777 == 0o600
        held = Ed25519PrivateKey.from_private_bytes((directory / 'seed').read_bytes()).public_key()
        assert held.public_bytes(Encoding.Raw, PublicFormat.Raw) == public_key(member)
        Ed25519PublicKey.from_public_bytes(public_key(member)).verify(
            bytes.fromhex(member['signature']),
            owner_native.guardian_binding_message(member['role'], member['identity'], 1, public_key(member)))
        assert member['identity'] == IDENTITIES[member['role']]
        assert member['epoch'] == 1 and member['rotation'] is None
        custody.add(member['custody'])
    assert len(custody) == 3
    keys = owner_native.guardian_set(document, root / 'recovery-guardians.json', 2)
    assert document['public_keys'] == sorted(key.hex() for key in keys)
    policy = json.loads((root / 'recovery-policy.json').read_text())
    assert policy['threshold'] == 2 and policy['delay_seconds'] == 86400
    assert bytes(policy['root']) == owner_native.guardian_commitment(2, keys)


def test_published_bindings_carry_no_custody_material(tmp_path):
    work, secrets = directories(tmp_path)
    document = enrolled_set(work, secrets)
    published = (work / 'human-evidence-input' / 'recovery-guardian-bindings.json').read_text()
    leaked = [member['role'] for member in document['members']
              if (Path(member['custody']) / 'seed').read_bytes().hex() in published]
    assert leaked == []


def test_enrolment_refuses_to_replace_existing_custody(tmp_path):
    work, secrets = directories(tmp_path)
    first = guardians.enroll(work, secrets, 'sequencer', IDENTITIES['sequencer'], 1)
    with pytest.raises(FileExistsError):
        guardians.enroll(work, secrets, 'sequencer', IDENTITIES['sequencer'], 1)
    seed = Path(first['custody']) / 'seed'
    held = Ed25519PrivateKey.from_private_bytes(seed.read_bytes()).public_key()
    assert held.public_bytes(Encoding.Raw, PublicFormat.Raw).hex() == first['public_key']


def test_enrolment_refuses_an_epoch_without_its_predecessor(tmp_path):
    work, secrets = directories(tmp_path)
    guardians.enroll(work, secrets, 'guarantor-1', IDENTITIES['guarantor-1'], 1)
    with pytest.raises(Refused):
        guardians.enroll(work, secrets, 'guarantor-1', IDENTITIES['guarantor-1'], 3)
    assert not guardians.custody_directory(secrets, 'guarantor-1', 3).exists()


def test_assembly_refuses_forged_substituted_and_duplicated_bindings(tmp_path):
    work, secrets = directories(tmp_path)
    document = enrolled_set(work, secrets)
    path = work / 'human-evidence-input' / 'recovery-guardians.json'
    assert owner_native.guardian_set(json.loads(json.dumps(document)), path, 2)

    forged = json.loads(json.dumps(document))
    forged['members'][0]['signature'] = '%0128x' % (int(forged['members'][0]['signature'], 16) ^ 1)
    with pytest.raises(Refused):
        owner_native.guardian_set(forged, path, 2)

    swapped = json.loads(json.dumps(document))
    keys = [member['public_key'] for member in swapped['members']]
    swapped['members'][0]['public_key'], swapped['members'][1]['public_key'] = keys[1], keys[0]
    with pytest.raises(Refused):
        owner_native.guardian_set(swapped, path, 2)

    duplicated = json.loads(json.dumps(document))
    duplicated['members'][1] = json.loads(json.dumps(duplicated['members'][0]))
    duplicated['public_keys'] = sorted(member['public_key'] for member in duplicated['members'])
    with pytest.raises(Refused):
        owner_native.guardian_set(duplicated, path, 2)

    manifest = json.loads(json.dumps(document))
    manifest['public_keys'] = manifest['public_keys'][:2] + [IDENTITIES['sequencer']]
    with pytest.raises(Refused):
        owner_native.guardian_set(manifest, path, 2)

    unrotated = json.loads(json.dumps(document))
    unrotated['members'][2]['rotation'] = dict(predecessor=unrotated['members'][0], signature='ab' * 64)
    with pytest.raises(Refused):
        owner_native.guardian_set(unrotated, path, 2)

    with pytest.raises(Refused):
        owner_native.guardian_set(json.loads(json.dumps(document)), path, 3)
    with pytest.raises(Refused):
        owner_native.guardian_set(dict(public_keys=document['public_keys']), path, 2)


def test_rotation_requires_the_outgoing_guardian_authorization(tmp_path):
    work, secrets = directories(tmp_path)
    genesis = enrolled_set(work, secrets)
    outgoing = next(member for member in genesis['members'] if member['role'] == 'sequencer')
    successor = guardians.enroll(work, secrets, 'sequencer', IDENTITIES['sequencer'], 2)
    rotated = work / 'human-evidence-input' / 'guardian-epoch-2'
    with pytest.raises(Refused):
        guardians.assemble(work, rotated)
    assert list(rotated.iterdir()) == []
    record = guardians.authorize(work, secrets, 'sequencer', 2)
    assert record['predecessor_public_key'] == outgoing['public_key']
    document = guardians.assemble(work, rotated)
    assert document['version'] == 2
    member = next(entry for entry in document['members'] if entry['role'] == 'sequencer')
    assert member['epoch'] == 2 and member['public_key'] == successor['public_key']
    predecessor = member['rotation']['predecessor']
    assert predecessor['public_key'] == outgoing['public_key'] and predecessor['epoch'] == 1
    assert predecessor['custody'] != member['custody']
    Ed25519PublicKey.from_public_bytes(public_key(predecessor)).verify(
        bytes.fromhex(member['rotation']['signature']),
        owner_native.guardian_rotation_message('sequencer', IDENTITIES['sequencer'], 2,
                                               public_key(predecessor), public_key(member)))
    keys = owner_native.guardian_set(document, rotated / 'recovery-guardians.json', 2)
    assert public_key(predecessor) not in keys and public_key(member) in keys
    policy = json.loads((rotated / 'recovery-policy.json').read_text())
    assert bytes(policy['root']) == owner_native.guardian_commitment(2, keys)
    assert bytes(policy['root']) != bytes(json.loads(
        (work / 'human-evidence-input' / 'recovery-policy.json').read_text())['root'])


def test_rotation_refuses_a_signature_from_another_key(tmp_path):
    work, secrets = directories(tmp_path)
    enrolled_set(work, secrets)
    guardians.enroll(work, secrets, 'sequencer', IDENTITIES['sequencer'], 2)
    guardians.authorize(work, secrets, 'sequencer', 2)
    document = guardians.assemble(work, work / 'human-evidence-input' / 'guardian-epoch-2')
    path = work / 'human-evidence-input' / 'guardian-epoch-2' / 'recovery-guardians.json'
    member = next(entry for entry in document['members'] if entry['role'] == 'sequencer')
    intruder = Ed25519PrivateKey.generate()
    forged = json.loads(json.dumps(document))
    rotation = next(entry for entry in forged['members'] if entry['role'] == 'sequencer')['rotation']
    rotation['signature'] = intruder.sign(owner_native.guardian_rotation_message(
        'sequencer', IDENTITIES['sequencer'], 2,
        bytes.fromhex(rotation['predecessor']['public_key']), public_key(member))).hex()
    with pytest.raises(Refused):
        owner_native.guardian_set(forged, path, 2)


def test_rotation_authorization_requires_the_outgoing_custody_directory(tmp_path):
    work, secrets = directories(tmp_path)
    enrolled_set(work, secrets)
    guardians.enroll(work, secrets, 'guarantor-2', IDENTITIES['guarantor-2'], 2)
    empty = tmp_path / 'other-secrets'
    empty.mkdir()
    with pytest.raises(Refused):
        guardians.authorize(work, empty, 'guarantor-2', 2)
    with pytest.raises(Refused):
        guardians.authorize(work, secrets, 'sequencer', 2)


def test_cli_refusals_report_without_a_traceback(tmp_path):
    work, secrets = directories(tmp_path)
    completed = subprocess.run([sys.executable, TOOL, 'enroll', '--work-dir', str(work), '--secrets-dir',
                                str(secrets), '--role', 'guarantor-1', '--identity', '00' * 32],
                               capture_output=True, text=True)
    assert completed.returncode != 0
    assert 'operator identity' in completed.stderr and 'Traceback' not in completed.stderr
    assert not (secrets / 'human-guardians').exists()
