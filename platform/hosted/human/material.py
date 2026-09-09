#!/usr/bin/env python3
import base64
import json
import os
from pathlib import Path
import re
import secrets
import stat
import sys
import unicodedata


def write(directory, name, value):
    path = directory / name
    with path.open('x', encoding='utf-8') as output:
        os.chmod(path, 0o600)
        output.write(str(value))


def protected_json(path):
    path = Path(path)
    info = path.lstat()
    if (not path.is_absolute() or path.resolve() != path
            or not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid()
            or stat.S_IMODE(info.st_mode) != 0o600 or info.st_nlink != 1 or info.st_size > 1048576):
        raise ValueError('Human evidence file ownership, type or bounds refused')
    return json.loads(path.read_text())


def assemble_policy(evidence, deployment, registry_path, output, network, chain):
    evidence = Path(evidence)
    deployment = json.loads(Path(deployment).read_text())
    registry = json.loads(Path(registry_path).read_text())
    if int(deployment['network_id']) != network or int(deployment['chain_id']) != chain:
        raise ValueError('Human deployment network mismatch')
    if registry.get('schema_version') != 2 or not registry.get('assets'):
        raise ValueError('Human requires the rendered version 2 module registry')
    policy = {key: protected_json(evidence / filename) for key, filename in {
        'components': 'components.json', 'agent': 'agent.json',
        'purpose_catalog': 'purpose-catalog.json', 'authority': 'authority.json',
        'principal_policy': 'principal-policy.json', 'recovery_policy': 'recovery-policy.json',
        'movement': 'movement-policy.json',
    }.items()}
    addresses = deployment['addresses']
    policy['components'].update({
        'PAXEER_EXIT_CONTRACT': addresses['emergency_exit'],
        'PAXEER_WITHDRAWAL_CLAIMS_CONTRACT': addresses['withdrawal_claims'],
    })
    policy['movement'].update({
        'PAXEER_VAULT': addresses['vault'],
        'PAXEER_CHECKPOINT_REGISTRY': addresses['checkpoint_registry'],
        'PAXEER_CLAIMS_CONTRACT': addresses['withdrawal_claims'],
        'PAXEER_EXIT_CONTRACT': addresses['emergency_exit'],
    })
    policy['registry'] = {'network_id': network, 'protocol_version': 3, 'modules': [
        {'module_id': module['module'], 'activity_types': [
            (module['module'] << 16) | ordinal for ordinal in module['ordinals']]}
        for module in registry['modules']]}
    policy['journal_directory'] = str(evidence / 'journal')
    write(Path(output).parent, Path(output).name, json.dumps(policy))


def main():
    root = Path(sys.argv[1])
    network, chain = int(sys.argv[2]), int(sys.argv[3])
    config = {
        'RP_ID': 'human.testnet.layerx.network', 'RP_NAME': 'LayerX Human',
        'ORIGIN': 'https://human.testnet.layerx.network',
        'CEREMONY_TTL_SECONDS': 300, 'ASSERTION_TTL_SECONDS': 60,
        'SESSION_TTL_SECONDS': 3600, 'REFRESH_TTL_SECONDS': 86400,
        'STEP_UP_TTL_SECONDS': 300, 'AUTH_RATE_ATTEMPTS': 5,
        'AUTH_RATE_WINDOW_SECONDS': 60, 'RETENTION_JOURNEYS_SECONDS': 2592000,
        'RETENTION_NOTIFICATIONS_SECONDS': 604800, 'RETENTION_AUDIT_SECONDS': 7776000,
        'RETENTION_TELEMETRY_SECONDS': 604800, 'RETENTION_CACHE_SECONDS': 300,
        'CAPABILITY_TTL_SECONDS': 30, 'AGENT_SOCKET': '/run/layerx/human/owner/agent.sock',
        'KMS_PROVIDER_REFERENCE': 'layerx-human-kms', 'KMS_ENDPOINT': '127.0.0.1:9450',
        'KMS_SERVER_NAME': 'layerx-human-kms',
        'KMS_ROOT_CERTIFICATE_DER': '/run/human-private/components/ca.der',
        'KMS_CLIENT_CERTIFICATE_DER': '/run/human-private/components/kms-client.der',
        'KMS_CLIENT_PRIVATE_KEY_DER': '/run/human-private/components/kms-client-key.der',
        'NETWORK_ID': network, 'PROTOCOL_VERSION': 3, 'SIGNING_RATE_MAXIMUM': 60,
        'SIGNING_RATE_WINDOW_SECONDS': 60, 'AGENT_TIMESTAMP_SPAN_SECONDS': 60,
        'AGENT_FEE_LIMIT': 1000, 'BINDING_STATEMENT_TTL_SECONDS': 300,
        'AGENT_PURPOSE_CATALOG': '/run/human-private/components/purpose-catalog.json',
        'PAXEER_RPC_URL': 'https://paxeer-boundary.layerx-testnet.svc.cluster.local:9443',
        'PAXEER_RPC_TIMEOUT_SECONDS': 10,
        'PAXEER_TRUST_ANCHOR_DER': '/run/human-private/components/ca.der',
        'PAXEER_CHAIN_ID': chain, 'EXIT_REQUIRED_CONFIRMATIONS': 12,
        'ACTIVITY_FRESHNESS_SECONDS': 60, 'ACTIVITY_EXPORT_MAXIMUM_BYTES': 1048576,
        'EXIT_POLL_CADENCE_SECONDS': 5, 'EXIT_DELAYED_AFTER_POLLS': 12,
        'CONTINUATION_UNKNOWN_DEADLINE_SECONDS': 300,
    }
    for name in ('TENANCY_DIGEST', 'AUTH_INDEX_KEY', 'STREAM_CURSOR_KEY'):
        config[name] = base64.urlsafe_b64encode(secrets.token_bytes(32)).decode().rstrip('=')
    for prefix in ('AGENT', 'KMS'):
        for key, value in {'MAX_FRAME_BYTES': 1048576, 'MAX_CONNECTIONS': 4,
                           'MAX_STREAMS': 4, 'MAX_QUEUED_BYTES': 4194304,
                           'DEADLINE_SECONDS': 10}.items():
            config[f'{prefix}_{key}'] = value
    for provider, uid in [('IDENTITY', 4020), ('SECURITY', 4020), ('MOVEMENT', 4020)]:
        config[f'{provider}_SOCKET'] = f'/run/layerx/human/{provider.lower()}.sock'
        config[f'{provider}_MAX_FRAME_BYTES'] = 1048576
        config[f'{provider}_DEADLINE_SECONDS'] = 10
        if provider != 'SECURITY':
            config[f'{provider}_PEER_UID'] = uid
            config[f'{provider}_PEER_GID'] = 4020
    agent = {
        'MODE': 'human-owner',
        'HUMAN_AUTHORITY_CA_DER': '/run/human-private/agent/ca.der',
        'AUTHORITY_CA_DER': '/run/human-private/agent/ca.der',
        'HUMAN_NODE_LNI': '/run/layerx/node/layerxd.lni.sock',
        'HUMAN_STORE': '/var/lib/layerx/human/agent/store',
        'HUMAN_SOCKET': '/run/layerx/human/owner/agent.sock',
        'HUMAN_SESSION_KEY_ROOT': '/var/lib/layerx/human/agent/sessions',
        'HUMAN_SESSION_OPERATOR_SECRET_FILE': '/run/human-private/agent/session-operator',
        'HUMAN_SOCKET_UID': 4021, 'HUMAN_SOCKET_GID': 4020, 'HUMAN_SOCKET_MODE': '0660',
        'HUMAN_NETWORK_ID': network, 'HUMAN_PROTOCOL_VERSION': 3,
        'HUMAN_DEADLINE_MS': 10000, 'HUMAN_MAX_FRAME_BYTES': 1048576,
        'HUMAN_MAX_CONNECTIONS': 4, 'HUMAN_MAX_STREAMS': 4,
        'HUMAN_MAX_QUEUED_BYTES': 4194304, 'HUMAN_MAX_PAYLOAD_BYTES': 1048576,
        'HUMAN_TIMESTAMP_SPAN': 60, 'HUMAN_RECONNECT_ATTEMPTS': 5,
        'HUMAN_RECONNECT_BASE_MS': 100, 'HUMAN_RECONNECT_MAX_MS': 2000,
        'HUMAN_RECONNECT_JITTER_PERCENT': 10,
        'HUMAN_AUTHORITY_ENDPOINT': 'https://layerx-receipt-authority.layerx-testnet.svc.cluster.local:9443',
        'HUMAN_AUTHORITY_MAX_BYTES': 1048576,
        'PROGRAM_LISTEN': '127.0.0.1:9451', 'PROGRAM_MAX_STALENESS_MS': 60000,
        'NODE_ENDPOINT': 'http://127.0.0.1:9401',
        'AUTHORITY_ENDPOINT': 'https://layerx-receipt-authority.layerx-testnet.svc.cluster.local:9443',
        'AUTHORITY_REPLICA_ID': (root.parent / 'receipt-authority-replica-id').read_text().strip(),
        'SEQUENCER_TRUST_HISTORY': '/run/human-private/agent/trust-history',
        'DEPLOYMENT_JOURNAL': '/run/human-private/agent/journal',
    }
    journal = root / 'journal'
    journal.mkdir(mode=0o700)
    if sys.argv[4]:
        policy = protected_json(sys.argv[4])
        required = {'components', 'agent', 'purpose_catalog', 'registry', 'journal_directory',
                    'authority', 'principal_policy', 'recovery_policy', 'movement'}
        if set(policy) != required:
            raise ValueError('Human policy fields do not match the documented contract')
        component_keys = {'AGENT_ACTOR', 'AGENT_AUTHORITY', 'AGENT_OWNER_ACCOUNT',
                          'AGENT_RECOVERY_ROOT', 'AGENT_RECOVERY_THRESHOLD',
                          'PAXEER_EXIT_CONTRACT', 'PAXEER_WITHDRAWAL_CLAIMS_CONTRACT'}
        agent_keys = {'HUMAN_PEERS', 'HUMAN_LIMIT_SCOPE', 'HUMAN_LIMIT_SCOPE_ID',
                      'HUMAN_LIMIT_ID', 'HUMAN_LIMIT_NAME', 'HUMAN_LIMIT_CEILING',
                      'HUMAN_LIMIT_CONSUMED'}
        for values, expected in [(policy['components'], component_keys), (policy['agent'], agent_keys)]:
            if set(values) != expected or any(not str(v) or '\n' in str(v) or '\0' in str(v) for v in values.values()):
                raise ValueError('Human policy binding fields refused')
        for name in ('PAXEER_EXIT_CONTRACT', 'PAXEER_WITHDRAWAL_CLAIMS_CONTRACT'):
            address = policy['components'][name]
            if not re.fullmatch(r'0x[0-9a-fA-F]{40}', address) or int(address, 16) == 0:
                raise ValueError('Human custody contract binding refused')
        if policy['registry']['network_id'] != network or policy['registry']['protocol_version'] != 3:
            raise ValueError('Human registry network or protocol mismatch')
        peers = policy['agent']['HUMAN_PEERS']
        if not isinstance(peers, str):
            raise ValueError('Human peer policy fields refused')
        peer = re.fullmatch(r'uid=(4020);tenant=([A-Za-z0-9_-]{1,128});principal=(did:[a-z0-9]+:[^;,]+)', peers)
        if peer is None:
            raise ValueError('Human peer policy must authorize only component UID 4020')
        tenant, principal = peer.group(2, 3)
        if (len(principal.encode('utf-8')) > 255
                or any(c.isspace() or unicodedata.category(c) == 'Cc' for c in principal)):
            raise ValueError('Human peer principal refused')
        authority = policy['authority']
        if set(authority) != {'tenant', 'principal', 'core-clock-horizon'}:
            raise ValueError('Human authority fields refused')
        if int(authority['core-clock-horizon']) <= 0:
            raise ValueError('Human core clock horizon refused')
        if tenant != authority['tenant'] or principal != authority['principal']:
            raise ValueError('Human authority peer binding differs')
        principals = policy['principal_policy']['principals']
        bound = [p for p in principals if p['tenant'] == authority['tenant']
                 and p['principal'] == authority['principal']]
        if len(bound) != 1:
            raise ValueError('Human authority principal binding missing')
        recovery = policy['recovery_policy']
        if (set(recovery) != {'root', 'threshold', 'delay_seconds'}
                or len(recovery['root']) != 32
                or any(type(b) is not int or not 0 <= b <= 255 for b in recovery['root'])
                or not any(recovery['root'])
                or not 1 <= recovery['threshold'] <= 65535 or recovery['delay_seconds'] <= 0
                or policy['components']['AGENT_RECOVERY_ROOT'] != base64.urlsafe_b64encode(
                    bytes(recovery['root'])).decode().rstrip('=')
                or int(policy['components']['AGENT_RECOVERY_THRESHOLD']) != recovery['threshold']):
            raise ValueError('Human recovery policy binding differs')
        movement = {
            'MODE': 'movement', 'ALLOWED_GID': 4020, 'MAX_FRAME_BYTES': 1048576,
            'DEADLINE_SECONDS': 5,
            'EVIDENCE_ROOT': '/var/lib/layerx/human/evidence',
            'PAXEER_RPC_URLS': json.dumps([
                'https://paxeer-boundary.layerx-testnet.svc.cluster.local:9443',
                'https://paxeer-observer-boundary.layerx-testnet.svc.cluster.local:9443']),
            'PAXEER_CA_DER': '/run/human-private/movement/ca.der',
            'PAXEER_CHAIN_ID': chain, 'PAXEER_MINIMUM_AGREEMENT': 2,
            'NETWORK_ID': network, 'PROTOCOL_VERSION': 3,
            'POLL_SECONDS': 5, 'DELAYED_AFTER_POLLS': 12,
            'KMS_ENDPOINT': '127.0.0.1:9450', 'KMS_SERVER_NAME': 'layerx-human-kms',
            'KMS_PROVIDER_REFERENCE': 'layerx-human-kms',
            'KMS_CA_DER': '/run/human-private/movement/ca.der',
            'KMS_CLIENT_CERT_DER': '/run/human-private/movement/kms-executor.der',
            'KMS_CLIENT_KEY_DER': '/run/human-private/movement/kms-executor-key.der',
        }
        movement_keys = {'PAXEER_VAULT', 'PAXEER_CHECKPOINT_REGISTRY',
                         'PAXEER_CLAIMS_CONTRACT', 'PAXEER_EXIT_CONTRACT',
                         'PAXEER_CHECKPOINT_AUTHORITY', 'CUSTODY_REFERENCE',
                         'PAXEER_CONFIRMATIONS', 'CHECKPOINT_INTERVAL_SECONDS',
                         'PAXEER_BLOCK_SECONDS', 'REMINDER_INTERVAL_SECONDS'}
        if set(policy['movement']) != movement_keys:
            raise ValueError('Human movement policy fields refused')
        for key, value in policy['movement'].items():
            width = 40 if key in {'PAXEER_VAULT', 'PAXEER_CHECKPOINT_REGISTRY',
                                 'PAXEER_CLAIMS_CONTRACT', 'PAXEER_EXIT_CONTRACT'} else 64
            if key in {'PAXEER_CONFIRMATIONS', 'CHECKPOINT_INTERVAL_SECONDS',
                       'PAXEER_BLOCK_SECONDS', 'REMINDER_INTERVAL_SECONDS'}:
                if type(value) is not int or value <= 0:
                    raise ValueError('Human movement timing refused')
            elif not re.fullmatch(r'0x[0-9a-fA-F]{' + str(width) + '}', value) or int(value, 16) == 0:
                raise ValueError('Human movement binding refused')
        if (policy['movement']['PAXEER_CLAIMS_CONTRACT'] != policy['components']['PAXEER_WITHDRAWAL_CLAIMS_CONTRACT']
                or policy['movement']['PAXEER_EXIT_CONTRACT'] != policy['components']['PAXEER_EXIT_CONTRACT']):
            raise ValueError('Human movement custody bindings differ')
        movement.update(policy['movement'])
        for key, value in movement.items():
            write(root / 'movement-config', 'LAYERX_HUMAN_MOVEMENT_PROVIDER_' + key, value)
        for key, value in authority.items():
            if not str(value) or any(c in str(value) for c in '\r\n\0'):
                raise ValueError('Human authority value refused')
            write(root / 'authority-config', key, value)
        write(root / 'authority', 'principal-policy.json', json.dumps(policy['principal_policy']))
        write(root / 'identity', 'recovery-policy.json', json.dumps(recovery))
        config['EXIT_REQUIRED_CONFIRMATIONS'] = movement['PAXEER_CONFIRMATIONS']
        config.update(policy['components'])
        agent.update(policy['agent'])
        write(root / 'components', 'purpose-catalog.json', json.dumps(policy['purpose_catalog']))
        write(root / 'kms', 'registry.json', json.dumps(policy['registry']))
        source = Path(policy['journal_directory'])
        if not source.is_absolute() or source.is_symlink() or not source.is_dir():
            raise ValueError('Human deployment journal directory refused')
        records = sorted(source.iterdir())
        if not records or len(records) > 128:
            raise ValueError('Human deployment journal record count refused')
        total = 0
        names = {record.name for record in records}
        for record in records:
            if not re.fullmatch(r'[0-9a-f]{64}\.(admission|deployment)', record.name):
                raise ValueError('Human deployment journal filename refused')
            if not {record.stem + '.admission', record.stem + '.deployment'} <= names:
                raise ValueError('Human deployment journal pair missing')
            info = record.lstat()
            if not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid() or info.st_mode & 0o022:
                raise ValueError('Human deployment journal file refused')
            total += info.st_size
            if not info.st_size or total > 524288:
                raise ValueError('Human deployment journal size refused')
            destination = journal / record.name
            destination.write_bytes(record.read_bytes())
            destination.chmod(0o600)
    for key, value in config.items():
        write(root / 'config', 'LAYERX_HUMAN_' + key, value)
    for key, value in agent.items():
        write(root / 'agent-config', 'LAYERX_AGENT_' + key, value)


if __name__ == '__main__':
    os.umask(0o077)
    try:
        if len(sys.argv) > 1 and sys.argv[1] == '--assemble':
            assemble_policy(*sys.argv[2:6], int(sys.argv[6]), int(sys.argv[7]))
        else:
            main()
    except (ValueError, OSError, KeyError, TypeError):
        raise SystemExit('Human material refused: check policy fields, file ownership and bounds')
