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



def purpose_catalog(template_path, registry_path, treasury_path, faucet_path, asset):
    template_path = Path(template_path)
    template = json.loads(template_path.read_text(), object_pairs_hook=strict_pairs)
    fields(template, 'version presets', template_path, 'catalog template')
    text(template['version'], template_path, 'version')
    array(template['presets'], template_path, 'presets')
    require(bool(template['presets']), template_path, 'presets')
    registry = protected_json(registry_path)
    require(type(registry) is dict and registry.get('schema_version') == 2,
            registry_path, 'version 2 module registry')
    h32(asset, registry_path, 'LAYERX_NODE_ASSET_ID')
    array(registry.get('assets'), registry_path, 'assets')
    require(any(type(a) is dict and a.get('asset') == asset for a in registry['assets']),
            registry_path, 'deployed asset registration')
    array(registry.get('modules'), registry_path, 'modules')
    activities = []
    for module in registry['modules']:
        require(type(module) is dict, registry_path, 'module')
        uint(module.get('module'), 16, registry_path, 'module', 1)
        require(module['module'] <= 9, registry_path, 'closed protocol module')
        array(module.get('ordinals'), registry_path, 'ordinals')
        for ordinal in module['ordinals']:
            uint(ordinal, 16, registry_path, 'ordinal', 1)
            activity = (module['module'] << 16) | ordinal
            require(activity not in activities, registry_path, 'duplicate activity')
            activities.append(activity)
    require(bool(activities), registry_path, 'registered activities')
    counterparties = []
    for path in (treasury_path, faucet_path):
        account = protected_json(path)
        require(type(account) is dict, path, 'account output')
        h32(account.get('account'), path, 'account')
        require(account['account'] not in counterparties, path, 'distinct protocol account')
        counterparties.append(account['account'])
    result = {'version': template['version'], 'presets': []}
    seen = set()
    for preset in template['presets']:
        fields(preset, 'id amount_ceiling rate_maximum_uses rate_window_sequences purposes expiry_sequence session_scopes session_lifetime_seconds budget_period_seconds budget_expiry_seconds initial_funding', template_path, 'preset template')
        text(preset['id'], template_path, 'preset.id')
        require(preset['id'] not in seen, template_path, 'duplicate preset')
        seen.add(preset['id'])
        for name in ('purposes', 'session_scopes'):
            array(preset[name], template_path, name)
            require(bool(preset[name]), template_path, name)
            for item in preset[name]:
                text(item, template_path, name)
            require(len(set(preset[name])) == len(preset[name]), template_path, name)
        for name in ('amount_ceiling', 'initial_funding'):
            uint(preset[name], 128, template_path, name, 1)
        for name in ('rate_maximum_uses', 'rate_window_sequences', 'expiry_sequence',
                     'session_lifetime_seconds', 'budget_period_seconds', 'budget_expiry_seconds'):
            uint(preset[name], 64, template_path, name, 1)
        result['presets'].append(dict(preset, activity_types=sorted(activities),
            counterparties=[list(bytes.fromhex(a)) for a in counterparties],
            assets=[list(bytes.fromhex(asset))], budget_asset=list(bytes.fromhex(asset))))
    return result


def write_json(path, value):
    path = Path(path)
    require(path.is_absolute() and path.resolve() == path, path, 'canonical absolute output')
    encoded = json.dumps(value, separators=(',', ':'), allow_nan=False) + '\n'
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, 'w') as output:
        output.write(encoded)
        output.flush()
        os.fsync(output.fileno())


def preserve_binding(request_path, response_path, output_path):
    request = protected_json(request_path)
    response = protected_json(response_path)
    require(type(request) is dict, request_path, 'principal request')
    require(type(response) is dict, response_path, 'principal response')
    text(request.get('tenant'), request_path, 'tenant absent from principal creation request')
    text(response.get('sub'), response_path, 'returned principal sub')
    require(request.get('sub') == response['sub'], response_path, 'requested principal binding')
    write_json(output_path, {'tenant': request['tenant'], 'principal': response['sub']})



def recovery_policy(value, path):
    fields(value, 'root threshold delay_seconds', path, 'recovery policy')
    array(value['root'], path, 'root')
    require(len(value['root']) == 32, path, 'root length')
    for byte in value['root']:
        uint(byte, 8, path, 'root byte')
    require(any(value['root']), path, 'nonzero root')
    uint(value['threshold'], 16, path, 'threshold', 1)
    uint(value['delay_seconds'], 64, path, 'delay_seconds', 1)


def job_input(work_dir):
    root = Path(work_dir) / 'human-evidence-input'
    path = root / 'owner-request.json'
    request = protected_json(path)
    require(path.stat().st_size <= 16384, path, 'LXIP request size')
    fields(request, 'email display_name idempotency_key now', path, 'owner request')
    for name in ('email', 'display_name', 'idempotency_key'):
        text(request[name], path, name)
    uint(request['now'], 64, path, 'now')
    path = root / 'recovery-policy.json'
    recovery_policy(protected_json(path), path)


def owner_result(work_dir, path):
    value = protected_json(path)
    require(len(Path(path).read_text().splitlines()) == 1, path, 'single-line LXIP result')
    fields(value, 'principal did recovery_root recovery_threshold recovery_delay_seconds', path, 'LXIP result')
    text(value['principal'], path, 'principal')
    text(value['did'], path, 'did')
    recovery = {'root': value['recovery_root'], 'threshold': value['recovery_threshold'],
                'delay_seconds': value['recovery_delay_seconds']}
    recovery_policy(recovery, path)
    policy_path = Path(work_dir) / 'human-evidence-input/recovery-policy.json'
    expected = protected_json(policy_path)
    recovery_policy(expected, policy_path)
    require(recovery == expected, path, 'LXIP recovery policy binding')
    return value


def main():
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument('--validate-owner-registration', action='store_true')
    mode.add_argument('--catalog', action='store_true')
    mode.add_argument('--validate-job-input', action='store_true')
    mode.add_argument('--validate-owner-result', action='store_true')
    mode.add_argument('--preserve-binding', action='store_true')
    parser.add_argument('--registry', type=Path)
    parser.add_argument('--treasury', type=Path)
    parser.add_argument('--faucet', type=Path)
    parser.add_argument('--asset')
    parser.add_argument('--request', type=Path)
    parser.add_argument('--response', type=Path)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--work-dir', type=Path, required=True)
    args = parser.parse_args()
    if args.validate_job_input:
        job_input(args.work_dir)
    elif args.validate_owner_result:
        require(args.request is not None, args.work_dir, 'owner result path')
        owner_result(args.work_dir, args.request)
    elif args.catalog:
        require(all((args.registry, args.treasury, args.faucet, args.asset, args.output)),
                args.work_dir, 'catalog input arguments')
        value = purpose_catalog(Path(__file__).with_name('beta-purpose-catalog.json'),
                                args.registry, args.treasury, args.faucet, args.asset)
        write_json(args.output, value)
    elif args.preserve_binding:
        require(all((args.request, args.response, args.output)), args.work_dir, 'binding arguments')
        preserve_binding(args.request, args.response, args.output)
    else:
        owner_registration(args.work_dir)


if __name__ == '__main__':
    try:
        main()
    except Refused as error:
        raise SystemExit(str(error))
