#!/usr/bin/env python3
import argparse
import base64
import shutil
import tempfile
import subprocess
import json
import os
from pathlib import Path
import re
import stat
import unicodedata
import time


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



def purpose_catalog(template_path, registry_path, treasury_path, sequencer_path, asset):
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
    for path in (treasury_path, sequencer_path):
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
    require(type(request['tenant']) is str and re.fullmatch(r'[a-z0-9_.-]{1,128}', request['tenant']) is not None, request_path, 'tenant')
    require(response.get('tenant') == request['tenant'], response_path, 'returned tenant binding')
    text(response.get('sub'), response_path, 'returned principal sub')
    require(request.get('sub') == response['sub'], response_path, 'requested principal binding')
    write_json(output_path, {'tenant': response['tenant'], 'principal': response['sub']})



def recovery_policy(value, path):
    fields(value, 'root threshold delay_seconds', path, 'recovery policy')
    array(value['root'], path, 'root')
    require(len(value['root']) == 32, path, 'root length')
    for byte in value['root']:
        uint(byte, 8, path, 'root byte')
    require(any(value['root']), path, 'nonzero root')
    uint(value['threshold'], 16, path, 'threshold', 1)
    uint(value['delay_seconds'], 64, path, 'delay_seconds', 1)


def owner_request(work_dir, secrets_dir):
    path = Path(secrets_dir) / 'owner-email'
    raw = protected_bytes(path, 320)
    try:
        email = raw.decode('utf-8').removesuffix('\n')
    except UnicodeDecodeError as error:
        raise Refused(f'{path}: invalid owner email encoding') from error
    require(bool(email) and email == email.strip() and email.count('@') == 1
            and not any(c.isspace() or unicodedata.category(c) == 'Cc' for c in email),
            path, 'owner email')
    output = Path(work_dir) / 'human-evidence-input/owner-request.json'
    try:
        write_json(output, {'email': email, 'display_name': 'Beta owner',
                            'idempotency_key': os.urandom(32).hex(), 'now': int(time.time())})
    except OSError as error:
        raise Refused(f'{output}: owner request publication refused; reconcile existing output') from error


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


def account_requests(work_dir):
    root = Path(work_dir)
    path = root / 'genesis/node.env'
    info = path.lstat()
    require(path.is_absolute() and path.resolve() == path and stat.S_ISREG(info.st_mode)
            and info.st_uid == os.geteuid() and info.st_nlink == 1 and not info.st_mode & 0o022
            and info.st_size <= 65536, path, 'protected bootstrap output')
    selected = {}
    names = {'LAYERX_NODE_TREASURY_DID', 'LAYERX_NODE_TREASURY_PUBLIC_KEY',
             'LAYERX_NODE_SEQUENCER_PUBLIC_KEY'}
    for line in path.read_text().splitlines():
        key, separator, value = line.partition('=')
        if key in names:
            require(separator and key not in selected, path, 'unique bootstrap binding')
            selected[key] = value
    require(set(selected) == names, path, 'treasury and sequencer exports')
    treasury = selected['LAYERX_NODE_TREASURY_PUBLIC_KEY']
    sequencer = selected['LAYERX_NODE_SEQUENCER_PUBLIC_KEY']
    h32(treasury, path, 'treasury public key')
    h32(sequencer, path, 'sequencer public key')
    require(treasury != sequencer, path, 'distinct counterparty keys')
    require(selected['LAYERX_NODE_TREASURY_DID'] == 'did:layerx:' + treasury, path, 'treasury DID')
    for name, key in [('treasury', treasury), ('sequencer', sequencer)]:
        write_json(root / 'human-evidence-input' / (name + '-request.json'), {'did': 'did:layerx:' + key})


def owner_policy():
    path = Path(__file__).with_name('beta-owner-policy.json')
    value = json.loads(path.read_text(), object_pairs_hook=strict_pairs)
    fields(value, 'core-clock-horizon maximum_age_seconds maximum_age_sequences limit activities_source budgets movement', path, 'owner policy')
    for name in ('core-clock-horizon', 'maximum_age_seconds', 'maximum_age_sequences'):
        uint(value[name], 64, path, name, 1)
    fields(value['limit'], 'scope scope_id_source id name ceiling', path, 'limit')
    require(value['limit']['scope'] == 'agent' and value['limit']['scope_id_source'] == 'owner_account', path, 'owner limit scope')
    h32(value['limit']['id'], path, 'limit.id')
    text(value['limit']['name'], path, 'limit.name')
    uint(value['limit']['ceiling'], 128, path, 'limit.ceiling', 1)
    require(value['activities_source'] == 'owner-registration', path, 'activities source')
    array(value['budgets'], path, 'budgets')
    require(len(set(value['budgets'])) == len(value['budgets']), path, 'duplicate budgets')
    for budget in value['budgets']:
        h32(budget, path, 'budget')
    fields(value['movement'], 'PAXEER_CONFIRMATIONS CHECKPOINT_INTERVAL_SECONDS PAXEER_BLOCK_SECONDS REMINDER_INTERVAL_SECONDS', path, 'movement intervals')
    for key, interval in value['movement'].items():
        uint(interval, 64, path, key, 1)
    return value


def principal_policy(binding, registration, asset, policy, path):
    entry = registration['identity']
    references = [entry['evidence'], entry['rotation']['evidence'], entry['recovery']['evidence']]
    references.extend(c['evidence'] for c in entry['capabilities'])
    activities = sorted({r['activity_id'] for r in references})
    identity(entry, path, activities)
    result = {'principals': [{'tenant': binding['tenant'], 'principal': binding['principal'],
        'account_id': registration['owner_account'], 'asset_id': asset,
        'activities': activities, 'budgets': policy['budgets'],
        'maximum_age_seconds': policy['maximum_age_seconds'],
        'maximum_age_sequences': policy['maximum_age_sequences'], 'identities': [entry]}]}
    encoded = json.dumps(result, separators=(',', ':'), allow_nan=False)
    parsed = json.loads(encoded, object_pairs_hook=strict_pairs)
    fields(parsed, 'principals', path, 'principal policy')
    require(len(parsed['principals']) == 1, path, 'single scoped principal')
    principal = parsed['principals'][0]
    fields(principal, 'tenant principal account_id asset_id activities budgets maximum_age_seconds maximum_age_sequences identities', path, 'principal policy entry')
    h32(principal['account_id'], path, 'account_id')
    h32(principal['asset_id'], path, 'asset_id')
    identity(principal['identities'][0], path, principal['activities'])
    require(parsed == result, path, 'principal policy reparse')
    return parsed


def protected_bytes(path, maximum=1048576):
    path = Path(path)
    fd = None
    try:
        require(path.is_absolute() and path.resolve() == path, path, 'canonical path')
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
        info = os.fstat(fd)
        require(stat.S_ISREG(info.st_mode) and info.st_uid == os.geteuid()
                and stat.S_IMODE(info.st_mode) == 0o600 and info.st_nlink == 1
                and 0 < info.st_size <= maximum, path, 'protected bounded file')
        with os.fdopen(fd, 'rb') as source:
            fd = None
            result = source.read(maximum + 1)
        require(0 < len(result) <= maximum, path, 'file bounds')
        return result
    except OSError as error:
        raise Refused(f'{path}: required protected file unavailable') from error
    finally:
        if fd is not None:
            os.close(fd)


def journal_records(path):
    require(path is not None, 'LAYERX_REGISTRY_JOURNAL', 'registry admission/deployment journal absent')
    path = Path(path)
    try:
        info = path.lstat()
        require(path.is_absolute() and path.resolve() == path and stat.S_ISDIR(info.st_mode)
                and info.st_uid == os.geteuid() and not info.st_mode & 0o022,
                path, 'protected registry journal directory')
        paths = sorted(path.iterdir())
    except OSError as error:
        raise Refused(f'{path}: registry admission/deployment journal unavailable') from error
    require(2 <= len(paths) <= 128, path, 'registry journal pair count')
    names = {p.name for p in paths}
    result = {}
    total = 0
    for record in paths:
        require(re.fullmatch(r'[0-9a-f]{64}\.(admission|deployment)', record.name) is not None,
                record, 'journal filename')
        require({record.stem + '.admission', record.stem + '.deployment'} <= names,
                record, 'registry journal pair missing')
        data = protected_bytes(record, 524288)
        total += len(data)
        require(total <= 524288, path, 'journal total size')
        result[record.name] = data
    return result


def assemble(work_dir, registry_path, asset, journal_path):
    work_dir = Path(work_dir)
    require(work_dir.is_absolute() and work_dir.resolve() == work_dir, work_dir, 'canonical work directory')
    destination = work_dir / 'human-evidence'
    require(not destination.exists() and not destination.is_symlink(), destination, 'existing evidence requires reconciliation')
    inputs = work_dir / 'human-evidence-input'
    owner = owner_result(work_dir, work_dir / 'human-owner-result.json')
    registration = owner_registration(work_dir, owner_did=owner['did'])
    binding_path = work_dir / 'identity/source-binding.json'
    binding = protected_json(binding_path)
    fields(binding, 'tenant principal', binding_path, 'tenant/principal binding')
    require(type(binding['tenant']) is str and re.fullmatch(r'[a-z0-9_.-]{1,128}', binding['tenant']) is not None,
            binding_path, 'tenant')
    text(binding['principal'], binding_path, 'principal')
    require(not any(c in binding['principal'] for c in ':,'), binding_path,
            'principal cannot be represented by the current HUMAN_PEERS delimiter parser')
    policy = owner_policy()
    catalog = purpose_catalog(Path(__file__).with_name('beta-purpose-catalog.json'), registry_path,
                              inputs / 'treasury.json', inputs / 'sequencer.json', asset)
    head_path = inputs / 'account-head-result.json'
    head = protected_json(head_path)
    fields(head, 'consumed', head_path, 'verified account head result')
    require(type(head['consumed']) is int and head['consumed'] == 0, head_path,
            'fresh first-batch head required; retained consumed limits unavailable')
    movement_path = inputs / 'movement-source.json'
    movement = protected_json(movement_path)
    fields(movement, 'CUSTODY_REFERENCE PAXEER_CHECKPOINT_AUTHORITY', movement_path, 'movement source')
    for key, value in movement.items():
        require(type(value) is str and re.fullmatch(r'0x[0-9a-fA-F]{64}', value) is not None
                and int(value, 16) != 0, movement_path, key)
    recovery = {'root': owner['recovery_root'], 'threshold': owner['recovery_threshold'],
                'delay_seconds': owner['recovery_delay_seconds']}
    files = {
        'components.json': {'AGENT_ACTOR': owner['did'], 'AGENT_AUTHORITY': registration['authority'],
            'AGENT_OWNER_ACCOUNT': 'agent:' + registration['identity']['did'] + ':main',
            'AGENT_RECOVERY_ROOT': base64.urlsafe_b64encode(bytes(recovery['root'])).decode().rstrip('='),
            'AGENT_RECOVERY_THRESHOLD': recovery['threshold']},
        'authority.json': dict(binding, **{'core-clock-horizon': policy['core-clock-horizon']}),
        'agent.json': {'HUMAN_PEERS': f"4020:{binding['principal']}:{binding['tenant']}",
            'HUMAN_LIMIT_SCOPE': policy['limit']['scope'], 'HUMAN_LIMIT_SCOPE_ID': registration['owner_account'],
            'HUMAN_LIMIT_ID': policy['limit']['id'], 'HUMAN_LIMIT_NAME': policy['limit']['name'],
            'HUMAN_LIMIT_CEILING': policy['limit']['ceiling'], 'HUMAN_LIMIT_CONSUMED': head['consumed']},
        'principal-policy.json': principal_policy(binding, registration, asset, policy,
                                                  inputs / 'owner-registration.json'),
        'recovery-policy.json': recovery, 'purpose-catalog.json': catalog,
        'movement-policy.json': dict(movement, **policy['movement'])}
    records = journal_records(journal_path)
    lock = work_dir / '.human-evidence-publish'
    try:
        lock.mkdir(mode=0o700)
    except FileExistsError as error:
        raise Refused(f'{lock}: publication already active or interrupted') from error
    pending = None
    try:
        require(not destination.exists() and not destination.is_symlink(), destination, 'existing evidence')
        pending = Path(tempfile.mkdtemp(prefix='.human-evidence-', dir=work_dir))
        for name, value in files.items():
            write_json(pending / name, value)
        (pending / 'journal').mkdir(mode=0o700)
        for name, data in records.items():
            fd = os.open(pending / 'journal' / name, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            with os.fdopen(fd, 'wb') as output:
                output.write(data)
                output.flush()
                os.fsync(output.fileno())
        for directory in (pending / 'journal', pending):
            fd = os.open(directory, os.O_RDONLY | os.O_DIRECTORY)
            try:
                os.fsync(fd)
            finally:
                os.close(fd)
        os.rename(pending, destination)
        pending = None
        fd = os.open(work_dir, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(fd)
        finally:
            os.close(fd)
    finally:
        if pending is not None:
            shutil.rmtree(pending)
        lock.rmdir()


def movement_source(work_dir, secrets_dir):
    inputs = Path(work_dir) / 'human-evidence-input'
    path = Path(secrets_dir) / 'human/movement-config/LAYERX_HUMAN_MOVEMENT_PROVIDER_CUSTODY_REFERENCE'
    if path.exists() or path.is_symlink():
        reference = protected_bytes(path, 66).decode('ascii')
    else:
        path = Path(work_dir) / 'paxeer/deployment.json'
        deployment = protected_json(path)
        require(type(deployment) is dict and 'custody_reference' in deployment, path,
                'custody_reference missing; vault registration requires a produced reference')
        reference = deployment['custody_reference']
    require(type(reference) is str and re.fullmatch(r'0x[0-9a-fA-F]{64}', reference) is not None
            and int(reference, 16) != 0, path, 'custody_reference')
    key_path = inputs / 'checkpoint-public.base64'
    try:
        public = base64.b64decode(protected_bytes(key_path, 128), validate=True).decode('ascii')
    except (ValueError, UnicodeError) as error:
        raise Refused('Secret layerx-guarantor-checkpoint-authority/public.hex: invalid public key') from error
    require(re.fullmatch(r'0x[0-9a-fA-F]{64}', public) is not None and int(public, 16) != 0,
            'Secret layerx-guarantor-checkpoint-authority/public.hex', 'checkpoint public key')
    write_json(inputs / 'movement-source.json', {'CUSTODY_REFERENCE': reference, 'PAXEER_CHECKPOINT_AUTHORITY': public})


def qualify_generated_set(work_dir, registry_path, secrets_dir, network, chain):
    import material
    work_dir = Path(work_dir)
    root = work_dir / 'human-material-check'
    require(not root.exists(), root, 'existing material check output')
    evidence = work_dir / 'human-evidence'
    for name in ('components', 'agent', 'authority', 'principal-policy', 'recovery-policy', 'purpose-catalog', 'movement-policy'):
        protected_json(evidence / (name + '.json'))
    journal_records(evidence / 'journal')
    root.mkdir(mode=0o700)
    private = root / 'human'
    private.mkdir(mode=0o700)
    for name in ('components', 'kms', 'config', 'agent-config', 'movement-config', 'authority-config', 'authority', 'identity'):
        (private / name).mkdir(mode=0o700)
    replica = protected_bytes(Path(secrets_dir) / 'receipt-authority-replica-id', 128).decode('ascii').strip()
    h32(replica, secrets_dir, 'receipt authority replica id')
    fd = os.open(root / 'receipt-authority-replica-id', os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, 'w') as output:
        output.write(replica)
    policy_path = root / 'policy.json'
    material.assemble_policy(evidence, work_dir / 'paxeer/deployment.json', registry_path,
                             policy_path, network, chain)
    subprocess.run(['python3', str(Path(__file__).with_name('material.py')), str(private),
                    str(network), str(chain), str(policy_path)], check=True)
    subprocess.run(['python3', str(Path(__file__).with_name('test_material.py'))], check=True)


def main():
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument('--prepare-owner-request', action='store_true')
    mode.add_argument('--validate-evidence-inputs', action='store_true')
    mode.add_argument('--validate-owner-registration', action='store_true')
    mode.add_argument('--produce-owner-registration', action='store_true')
    mode.add_argument('--prepare-owner-admission', action='store_true')
    mode.add_argument('--catalog', action='store_true')
    mode.add_argument('--assemble', action='store_true')
    mode.add_argument('--movement-source', action='store_true')
    mode.add_argument('--qualify-generated-set', action='store_true')
    mode.add_argument('--account-requests', action='store_true')
    mode.add_argument('--validate-job-input', action='store_true')
    mode.add_argument('--validate-owner-result', action='store_true')
    mode.add_argument('--preserve-binding', action='store_true')
    parser.add_argument('--registry', type=Path)
    parser.add_argument('--treasury', type=Path)
    parser.add_argument('--sequencer', type=Path)
    parser.add_argument('--asset')
    parser.add_argument('--journal', type=Path)
    parser.add_argument('--secrets-dir', type=Path)
    parser.add_argument('--network', type=int)
    parser.add_argument('--chain', type=int)
    parser.add_argument('--request', type=Path)
    parser.add_argument('--response', type=Path)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--work-dir', type=Path, required=True)
    args = parser.parse_args()
    if args.prepare_owner_request:
        require(args.secrets_dir is not None, args.work_dir, 'secrets directory')
        owner_request(args.work_dir, args.secrets_dir)
    elif args.prepare_owner_admission:
        require(args.secrets_dir is not None, args.work_dir, 'secrets directory')
        from owner_native import prepare_admission
        prepare_admission(args.work_dir, args.secrets_dir)
    elif args.produce_owner_registration:
        from owner_native import produce
        produce(args.work_dir)
    elif args.qualify_generated_set:
        require(all((args.registry, args.secrets_dir, args.network, args.chain)), args.work_dir, 'generated set qualification arguments')
        qualify_generated_set(args.work_dir, args.registry, args.secrets_dir, args.network, args.chain)
    elif args.movement_source:
        require(args.secrets_dir is not None, args.work_dir, 'secrets directory')
        movement_source(args.work_dir, args.secrets_dir)
    elif args.assemble:
        require(all((args.registry, args.asset)), args.work_dir, 'registry and asset arguments')
        assemble(args.work_dir, args.registry, args.asset, args.journal)
    elif args.account_requests:
        account_requests(args.work_dir)
    elif args.validate_job_input:
        job_input(args.work_dir)
    elif args.validate_owner_result:
        require(args.request is not None, args.work_dir, 'owner result path')
        owner_result(args.work_dir, args.request)
    elif args.catalog:
        require(all((args.registry, args.treasury, args.sequencer, args.asset, args.output)),
                args.work_dir, 'catalog input arguments')
        value = purpose_catalog(Path(__file__).with_name('beta-purpose-catalog.json'),
                                args.registry, args.treasury, args.sequencer, args.asset)
        write_json(args.output, value)
    elif args.preserve_binding:
        require(all((args.request, args.response, args.output)), args.work_dir, 'binding arguments')
        preserve_binding(args.request, args.response, args.output)
    else:
        owner_registration(args.work_dir)


if __name__ == '__main__':
    import sys
    sys.modules['provision'] = sys.modules[__name__]
    try:
        main()
    except Refused as error:
        raise SystemExit(str(error))
