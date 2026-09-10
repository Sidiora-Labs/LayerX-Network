import argparse
from pathlib import Path

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PrivateFormat, PublicFormat, NoEncryption

from owner_native import (GUARDIAN_EPOCH_MAXIMUM, GUARDIAN_ROLES, guardian_binding_message, guardian_commitment,
                          guardian_enrollment_key, guardian_rotation_message, guardian_set, protected_write)
from provision import fields, h32, protected_bytes, protected_json, require, uint, write_json

THRESHOLD = 2
DELAY_SECONDS = 86400


def custody_directory(secrets_dir, role, epoch):
    return Path(secrets_dir) / 'human-guardians' / f'{role}-e{epoch}'


def enrollment_path(root, role, epoch):
    return root / f'guardian-enrollment-{role}-e{epoch}.json'


def rotation_record_path(root, role, epoch):
    return root / f'guardian-rotation-{role}-e{epoch}.json'


def evidence_root(work_dir):
    return (Path(work_dir) / 'human-evidence-input').resolve()


def enroll(work_dir, secrets_dir, role, identity, epoch):
    root = evidence_root(work_dir)
    require(role in GUARDIAN_ROLES, root, 'guardian role')
    h32(identity, root, 'operator identity')
    uint(epoch, 16, root, 'guardian epoch', 1)
    require(epoch <= GUARDIAN_EPOCH_MAXIMUM, root, 'guardian epoch bound')
    root.mkdir(mode=0o700, exist_ok=True)
    if epoch > 1:
        previous_path = enrollment_path(root, role, epoch - 1)
        previous = protected_json(previous_path)
        guardian_enrollment_key(previous, previous_path)
        require(previous['role'] == role and previous['epoch'] == epoch - 1 and previous['identity'] == identity,
                previous_path, 'guardian rotation identity')
    custody = custody_directory(secrets_dir, role, epoch)
    custody.parent.mkdir(mode=0o700, exist_ok=True)
    custody.mkdir(mode=0o700)
    key = Ed25519PrivateKey.generate()
    public = key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    protected_write(custody / 'seed', key.private_bytes(Encoding.Raw, PrivateFormat.Raw, NoEncryption()))
    enrollment = dict(role=role, identity=identity, public_key=public.hex(), epoch=epoch,
                      custody=str(custody.resolve()),
                      signature=key.sign(guardian_binding_message(role, identity, epoch, public)).hex())
    write_json(enrollment_path(root, role, epoch), enrollment)
    return enrollment


def authorize(work_dir, secrets_dir, role, epoch):
    root = evidence_root(work_dir)
    require(role in GUARDIAN_ROLES, root, 'guardian role')
    uint(epoch, 16, root, 'guardian rotation epoch', 2)
    require(epoch <= GUARDIAN_EPOCH_MAXIMUM, root, 'guardian epoch bound')
    successor_path = enrollment_path(root, role, epoch)
    successor = protected_json(successor_path)
    successor_key = guardian_enrollment_key(successor, successor_path)
    predecessor_path = enrollment_path(root, role, epoch - 1)
    predecessor = protected_json(predecessor_path)
    predecessor_key = guardian_enrollment_key(predecessor, predecessor_path)
    require(successor['role'] == role and successor['epoch'] == epoch and predecessor['role'] == role
            and predecessor['epoch'] == epoch - 1 and successor['identity'] == predecessor['identity']
            and successor_key != predecessor_key, successor_path, 'guardian rotation successor')
    seed_path = custody_directory(secrets_dir, role, epoch - 1).resolve() / 'seed'
    seed = protected_bytes(seed_path, 32)
    require(len(seed) == 32, seed_path, 'guardian custody Ed25519 seed')
    key = Ed25519PrivateKey.from_private_bytes(seed)
    require(key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw) == predecessor_key,
            seed_path, 'outgoing guardian custody key binding')
    identity = predecessor['identity']
    record = dict(role=role, identity=identity, epoch=epoch, predecessor_public_key=predecessor['public_key'],
                  public_key=successor['public_key'],
                  signature=key.sign(guardian_rotation_message(role, identity, epoch, predecessor_key,
                                                               successor_key)).hex())
    write_json(rotation_record_path(root, role, epoch), record)
    return record


def active_epoch(root, role):
    epoch = 0
    while epoch < GUARDIAN_EPOCH_MAXIMUM and enrollment_path(root, role, epoch + 1).exists():
        epoch += 1
    require(epoch > 0, enrollment_path(root, role, 1), 'guardian enrollment')
    return epoch


def enrolled(root, role, epoch):
    path = enrollment_path(root, role, epoch)
    enrollment = protected_json(path)
    guardian_enrollment_key(enrollment, path)
    require(enrollment['role'] == role and enrollment['epoch'] == epoch, path, 'guardian enrollment binding')
    member = dict(enrollment, rotation=None)
    if epoch > 1:
        record_path = rotation_record_path(root, role, epoch)
        record = protected_json(record_path)
        fields(record, 'role identity epoch predecessor_public_key public_key signature', record_path,
               'guardian rotation record')
        predecessor = enrolled(root, role, epoch - 1)
        require(record['role'] == role and record['epoch'] == epoch and record['identity'] == member['identity']
                and record['public_key'] == member['public_key']
                and record['predecessor_public_key'] == predecessor['public_key'],
                record_path, 'guardian rotation record binding')
        member['rotation'] = dict(predecessor=predecessor, signature=record['signature'])
    return member


def assemble(work_dir, output_dir=None):
    root = evidence_root(work_dir)
    output = Path(output_dir).resolve() if output_dir is not None else root
    output.mkdir(mode=0o700, exist_ok=True)
    members = [enrolled(root, role, active_epoch(root, role)) for role in GUARDIAN_ROLES]
    document = dict(version=max(member['epoch'] for member in members), threshold=THRESHOLD,
                    public_keys=sorted(member['public_key'] for member in members), members=members)
    guardians_path = output / 'recovery-guardians.json'
    keys = guardian_set(document, guardians_path, THRESHOLD)
    write_json(guardians_path, document)
    write_json(output / 'recovery-guardian-bindings.json', document)
    write_json(output / 'recovery-policy.json', dict(root=list(guardian_commitment(THRESHOLD, keys)),
                                                     threshold=THRESHOLD, delay_seconds=DELAY_SECONDS))
    return document


def main():
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(dest='command', required=True)
    enrollment = commands.add_parser('enroll')
    enrollment.add_argument('--work-dir', required=True)
    enrollment.add_argument('--secrets-dir', required=True)
    enrollment.add_argument('--role', required=True)
    enrollment.add_argument('--identity', required=True)
    enrollment.add_argument('--epoch', type=int, default=1)
    authorization = commands.add_parser('authorize')
    authorization.add_argument('--work-dir', required=True)
    authorization.add_argument('--secrets-dir', required=True)
    authorization.add_argument('--role', required=True)
    authorization.add_argument('--epoch', type=int, required=True)
    assembly = commands.add_parser('assemble')
    assembly.add_argument('--work-dir', required=True)
    assembly.add_argument('--output-dir')
    args = parser.parse_args()
    if args.command == 'enroll':
        enroll(args.work_dir, args.secrets_dir, args.role, args.identity, args.epoch)
    elif args.command == 'authorize':
        authorize(args.work_dir, args.secrets_dir, args.role, args.epoch)
    else:
        assemble(args.work_dir, args.output_dir)


if __name__ == '__main__':
    import sys

    from provision import Refused
    sys.modules['guardians'] = sys.modules[__name__]
    try:
        main()
    except Refused as error:
        raise SystemExit(str(error))
