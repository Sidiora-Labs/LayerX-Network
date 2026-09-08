#!/usr/bin/env python3
import argparse
import json
import os
from pathlib import Path
import re
import stat


class Refused(ValueError):
    pass


def require(condition, path, field):
    if not condition:
        raise Refused(f'{path}: invalid {field}')


def fields(value, names, path, field):
    require(type(value) is dict and set(value) == set(names.split()), path, field)


def uint(value, bits, path, field, minimum=0):
    require(type(value) is int and minimum <= value < 1 << bits, path, field)


def text(value, path, field):
    require(type(value) is str and bool(value) and not any(ord(c) < 32 or ord(c) == 127 for c in value), path, field)


def h32(value, path, field):
    require(type(value) is str and re.fullmatch('[0-9a-f]{64}', value) is not None
            and int(value, 16) != 0, path, field)


def array(value, path, field):
    require(type(value) is list, path, field)


def strict_pairs(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError('duplicate JSON field')
        result[key] = value
    return result


def protected_json(path):
    path = Path(path)
    fd = None
    try:
        require(path.is_absolute() and path.resolve() == path, path, 'canonical absolute path')
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
        info = os.fstat(fd)
        require(stat.S_ISREG(info.st_mode) and info.st_uid == os.geteuid()
                and stat.S_IMODE(info.st_mode) == 0o600 and info.st_nlink == 1
                and 0 < info.st_size <= 1048576, path, 'protected regular file (0600, owner, single link, <=1 MiB)')
        with os.fdopen(fd, 'rb') as source:
            fd = None
            data = source.read(1048577)
        require(len(data) <= 1048576, path, 'JSON size')
        return json.loads(data, object_pairs_hook=strict_pairs,
                          parse_constant=lambda _: (_ for _ in ()).throw(ValueError('nonfinite JSON')))
    except Refused:
        raise
    except (OSError, ValueError) as error:
        raise Refused(f'{path}: required protected JSON unavailable or invalid') from error
    finally:
        if fd is not None:
            os.close(fd)


def evidence(value, path, field, activities):
    fields(value, 'activity_id receipt_digest', path, field)
    for key in value:
        h32(value[key], path, f'{field}.{key}')
    if activities is not None:
        require(value['activity_id'] in activities, path, f'{field}.activity_id binding')


def key_policy(value, path, field, activities):
    fields(value, 'policy_revision required_delay_seconds maximum_delay_seconds effective_sequence evidence', path, field)
    for key in ('policy_revision', 'required_delay_seconds', 'effective_sequence'):
        uint(value[key], 64, path, f'{field}.{key}', 1)
    uint(value['maximum_delay_seconds'], 64, path, f'{field}.maximum_delay_seconds', value['required_delay_seconds'])
    evidence(value['evidence'], path, f'{field}.evidence', activities)


def identity(value, path, activities=None):
    fields(value, 'did authorities revocation_sequence frozen evidence capabilities rotation recovery', path, 'identity')
    text(value['did'], path, 'identity.did')
    uint(value['revocation_sequence'], 64, path, 'identity.revocation_sequence', 1)
    require(type(value['frozen']) is bool, path, 'identity.frozen')
    array(value['authorities'], path, 'identity.authorities')
    require(bool(value['authorities']), path, 'identity.authorities')
    for authority in value['authorities']:
        fields(authority, 'kind id', path, 'identity.authorities[]')
        require(authority['kind'] in ('primary_key', 'session_key', 'capability_grant'), path, 'authority.kind')
        h32(authority['id'], path, 'authority.id')
    evidence(value['evidence'], path, 'identity.evidence', activities)
    array(value['capabilities'], path, 'identity.capabilities')
    seen = set()
    for capability in value['capabilities']:
        fields(capability, 'authority action_key capability_id activity_types counterparties assets amount_ceiling expiry_sequence enforceable_dimensions evidence', path, 'identity.capabilities[]')
        for key in ('authority', 'action_key', 'capability_id'):
            h32(capability[key], path, f'capability.{key}')
        binding = tuple(capability[k] for k in ('authority', 'action_key', 'capability_id'))
        require(binding not in seen, path, 'duplicate capability binding')
        seen.add(binding)
        require(any(a['id'] == capability['authority'] for a in value['authorities']), path, 'capability authority binding')
        for key in ('activity_types', 'counterparties', 'assets', 'enforceable_dimensions'):
            array(capability[key], path, f'capability.{key}')
        for activity in capability['activity_types']:
            uint(activity, 16, path, 'capability.activity_types[]')
        for key in ('counterparties', 'assets'):
            for item in capability[key]:
                h32(item, path, f'capability.{key}[]')
        amount = capability['amount_ceiling']
        require(type(amount) is str and re.fullmatch('[0-9]+', amount) is not None
                and len(amount) <= 39 and int(amount) < 1 << 128, path, 'capability.amount_ceiling')
        uint(capability['expiry_sequence'], 64, path, 'capability.expiry_sequence', 1)
        require(all(type(d) is str and d in ('activity_type', 'counterparty', 'asset', 'amount', 'rate', 'purpose', 'expiry') for d in capability['enforceable_dimensions']), path, 'capability.enforceable_dimensions')
        evidence(capability['evidence'], path, 'capability.evidence', activities)
    for key in ('rotation', 'recovery'):
        key_policy(value[key], path, f'identity.{key}', activities)


def owner_registration(work_dir, activities=None, owner_did=None):
    path = Path(work_dir) / 'human-evidence-input/owner-registration.json'
    value = protected_json(path)
    fields(value, 'owner_account authority identity', path, 'owner registration')
    h32(value['owner_account'], path, 'owner_account')
    text(value['authority'], path, 'authority')
    identity(value['identity'], path, activities)
    if owner_did is not None:
        require(value['identity']['did'] == owner_did, path, 'LXIP owner DID binding')
    return value


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--validate-owner-registration', action='store_true', required=True)
    parser.add_argument('--work-dir', type=Path, required=True)
    args = parser.parse_args()
    owner_registration(args.work_dir)


if __name__ == '__main__':
    try:
        main()
    except Refused as error:
        raise SystemExit(str(error))
