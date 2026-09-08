#!/usr/bin/env python3
import base64
import json
import os
from pathlib import Path
import re
import secrets
import stat
import sys


def write(directory, name, value):
    path = directory / name
    with path.open('x', encoding='utf-8') as output:
        os.chmod(path, 0o600)
        output.write(str(value))


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
        'CAPABILITY_TTL_SECONDS': 30, 'AGENT_SOCKET': '/run/layerx/human/agent.sock',
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
        'HUMAN_NODE_LNI': '/run/layerx/node/layerxd.lni.sock',
        'HUMAN_STORE': '/var/lib/layerx/human/agent/store',
        'HUMAN_SOCKET': '/run/layerx/human/agent.sock',
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
        policy_path = Path(sys.argv[4])
        metadata = policy_path.lstat()
        if (not policy_path.is_absolute() or not stat.S_ISREG(metadata.st_mode)
                or metadata.st_uid != os.geteuid() or metadata.st_mode & 0o077
                or metadata.st_size > 1048576):
            raise ValueError('Human policy file ownership, type or bounds refused')
        policy = json.loads(policy_path.read_text())
        required = {'components', 'agent', 'purpose_catalog', 'registry', 'journal_directory'}
        if set(policy) != required:
            raise ValueError('Human policy fields do not match the documented contract')
        component_keys = {'AGENT_ACTOR', 'AGENT_AUTHORITY', 'AGENT_OWNER_ACCOUNT',
                          'AGENT_RECOVERY_ROOT', 'AGENT_RECOVERY_THRESHOLD',
                          'PAXEER_EXIT_CONTRACT', 'PAXEER_WITHDRAWAL_CLAIMS_CONTRACT'}
        agent_keys = {'HUMAN_PEERS', 'HUMAN_LIMIT_SCOPE', 'HUMAN_LIMIT_SCOPE_ID',
                      'HUMAN_LIMIT_ID', 'HUMAN_LIMIT_NAME', 'HUMAN_LIMIT_CEILING',
                      'HUMAN_LIMIT_CONSUMED', 'PROGRAM_PROBE_ID'}
        for values, expected in [(policy['components'], component_keys), (policy['agent'], agent_keys)]:
            if set(values) != expected or any(not str(v) or '\n' in str(v) or '\0' in str(v) for v in values.values()):
                raise ValueError('Human policy binding fields refused')
        for name in ('PAXEER_EXIT_CONTRACT', 'PAXEER_WITHDRAWAL_CLAIMS_CONTRACT'):
            address = policy['components'][name]
            if not re.fullmatch(r'0x[0-9a-fA-F]{40}', address) or int(address, 16) == 0:
                raise ValueError('Human custody contract binding refused')
        if policy['registry']['network_id'] != network or policy['registry']['protocol_version'] != 3:
            raise ValueError('Human registry network or protocol mismatch')
        peers = policy['agent']['HUMAN_PEERS'].split(',')
        if len(peers) != 1 or len(peers[0].split(':', 2)) != 3 or not peers[0].startswith('4020:'):
            raise ValueError('Human peer policy must authorize only component UID 4020')
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
        main()
    except (ValueError, OSError, KeyError, TypeError):
        raise SystemExit('Human material refused: check policy fields, file ownership and bounds')
