import copy
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
script = (ROOT / 'platform/hosted/tests/topology-check.sh').read_text()
code = script.split("<<'PY'\n", 1)[1].split('\nPY\n', 1)[0]
module = {}
exec(compile(code.rsplit('sys.exit(main())', 1)[0], str(Path('platform/hosted/tests/topology-check.sh')), 'exec'), module)
paths = ['node', 'identity', 'paxeer', 'testnet', 'gateway', 'registry', 'internal', 'webhooks']

RESOLVED_EDGES = (
    'Deployment layerx-testnet/layerx-testnet-control -> layerx-pending-core.layerx-testnet.svc.cluster.local:9443 [env LAYERX_TESTNET_CORE_URL]',
    'Deployment layerx-testnet/layerx-testnet-control -> layerx-pending-core-admin.layerx-testnet.svc.cluster.local:9444 [env LAYERX_TESTNET_CORE_ADMIN_URL]',
    'Deployment layerx-testnet/layerx-testnet-control -> layerx-gateway.layerx-testnet.svc.cluster.local:443 [env LAYERX_TESTNET_GATEWAY_URL]',
    'Deployment layerx-testnet/layerx-testnet-control -> paxeer-boundary.layerx-testnet.svc.cluster.local:9443 [env LAYERX_TESTNET_PAXEER_URL]',
    'Deployment layerx-testnet/layerx-testnet-control -> layerx-identity.layerx-testnet.svc.cluster.local:9443 [env LAYERX_TESTNET_IDENTITY_URL]',
    'Deployment layerx-testnet/layerx-testnet-control -> layerx-faucet-public.layerx-testnet.svc.cluster.local:443 [env LAYERX_TESTNET_FAUCET_URL]',
    'Deployment layerx-testnet/layerx-testnet-control -> layerx-receipt-authority.layerx-testnet.svc.cluster.local:9443 [env LAYERX_TESTNET_RECEIPT_AUTHORITY_URL]',
    'Deployment layerx-testnet/layerx-testnet-control -> layerx-program-registry.layerx-testnet.svc.cluster.local:9420 [env LAYERX_TESTNET_REGISTRY_URL]',
    'Deployment layerx-testnet/layerx-testnet-control -> layerx-faucet-redis.layerx-testnet.svc.cluster.local:6379 [env LAYERX_TESTNET_REDIS_URL]',
    'Deployment layerx-testnet/layerx-faucet -> layerx-identity.layerx-testnet.svc.cluster.local:9443 [env LAYERX_IDENTITY_INTROSPECTION_URL]',
    'Deployment layerx-testnet/layerx-faucet -> layerx-testnet-admin.layerx-testnet.svc.cluster.local:443 [env LAYERX_TESTNET_FUNDING_URL]',
    'Deployment layerx-testnet/layerx-faucet -> layerx-faucet-redis.layerx-testnet.svc.cluster.local:6379 [env LAYERX_FAUCET_REDIS_URL]',
    'CronJob layerx-testnet/layerx-testnet-reset -> layerx-testnet-admin.layerx-testnet.svc.cluster.local:443 [env LAYERX_TESTNET_ADMIN_URL]',
    'CronJob layerx-testnet/layerx-testnet-status-publisher -> layerx-testnet-public.layerx-testnet.svc.cluster.local:443 [env LAYERX_TESTNET_STATUS_URL]',
    'Deployment layerx-testnet/layerx-gateway -> layerx-pending-core.layerx-testnet.svc.cluster.local:9443 [env LAYERX_GATEWAY_PUBLIC_CORE_URL]',
    'Deployment layerx-testnet/layerx-gateway -> layerx-agent-boundary.layerx-testnet.svc.cluster.local:9443 [env LAYERX_GATEWAY_COMPONENT_URL]',
    'Deployment layerx-testnet/layerx-gateway -> layerx-receipt-authority.layerx-testnet.svc.cluster.local:9443 [env LAYERX_GATEWAY_AUTHORITY_URL]',
    'Deployment layerx-testnet/layerx-gateway -> layerx-identity.layerx-testnet.svc.cluster.local:9443 [env LAYERX_GATEWAY_IDENTITY_URL]',
    'Deployment layerx-testnet/layerx-gateway -> layerx-program-registry.layerx-testnet.svc.cluster.local:9420 [env LAYERX_GATEWAY_PROGRAM_REGISTRY_URL]',
    'Deployment layerx-testnet/layerx-gateway -> layerx-gateway-redis.layerx-testnet.svc.cluster.local:6379 [env LAYERX_GATEWAY_REDIS_URL]',
    'StatefulSet layerx-testnet/layerx-program-registry -> layerx-agent-boundary.layerx-testnet.svc.cluster.local:9443 [env LAYERX_REGISTRY_NODE_ENDPOINT]',
    'StatefulSet layerx-testnet/layerx-program-registry -> layerx-receipt-authority.layerx-testnet.svc.cluster.local:9443 [env LAYERX_REGISTRY_RECEIPT_AUTHORITY_ENDPOINT]',
    'Deployment layerx-internal/payments -> layerx-gateway.layerx-testnet.svc:443 [env LAYERX_EVENTS_UPSTREAM_URL]',
    'Deployment layerx-internal/programs -> layerx-gateway.layerx-testnet.svc:443 [env LAYERX_EVENTS_UPSTREAM_URL]',
    'Deployment layerx-developer/layerx-webhooks -> layerx-agent-boundary.layerx-testnet.svc:9443 [env LAYERX_WEBHOOKS_COMPONENT_URL]',
    'Deployment layerx-developer/layerx-webhooks -> redis.layerx-internal.svc:6379 [env LAYERX_WEBHOOKS_REDIS_URL (ConfigMap layerx-developer-hosted)]',
    'Deployment layerx-developer/layerx-webhooks -> kms.layerx-internal.svc:443 [env LAYERX_WEBHOOKS_KMS_URL (ConfigMap layerx-developer-hosted)]',
    'Deployment layerx-developer/layerx-webhooks -> identity.layerx-internal.svc:443 [env LAYERX_WEBHOOKS_IDENTITY_URL (ConfigMap layerx-developer-hosted)]',
    'Deployment layerx-developer/layerx-webhooks -> authority.layerx-internal.svc:443 [env LAYERX_WEBHOOKS_AUTHORITY_URL (ConfigMap layerx-developer-hosted)]',
    'Deployment layerx-developer/layerx-webhooks -> journeys.layerx-internal.svc:443 [env LAYERX_WEBHOOKS_JOURNEY_SOURCE_URL (ConfigMap layerx-developer-hosted)]',
    'Deployment layerx-developer/layerx-webhooks -> payments.layerx-internal.svc:443 [env LAYERX_WEBHOOKS_PAYMENT_SOURCE_URL (ConfigMap layerx-developer-hosted)]',
    'Deployment layerx-developer/layerx-webhooks -> approvals.layerx-internal.svc:443 [env LAYERX_WEBHOOKS_APPROVAL_SOURCE_URL (ConfigMap layerx-developer-hosted)]',
    'Deployment layerx-developer/layerx-webhooks -> programs.layerx-internal.svc:443 [env LAYERX_WEBHOOKS_PROGRAM_SOURCE_URL (ConfigMap layerx-developer-hosted)]',
    'Deployment layerx-developer/layerx-dashboard-api -> redis.layerx-internal.svc:6379 [env LAYERX_WEBHOOKS_REDIS_URL (ConfigMap layerx-dashboard-hosted)]',
    'Deployment layerx-developer/layerx-dashboard-api -> layerx-gateway-redis.layerx-testnet.svc.cluster.local:6379 [env LAYERX_DASHBOARD_GATEWAY_REDIS_URL (ConfigMap layerx-dashboard-hosted)]',
    'Deployment layerx-developer/layerx-dashboard-api -> identity.layerx-internal.svc:443 [env LAYERX_DASHBOARD_IDENTITY_URL (ConfigMap layerx-dashboard-hosted)]',
    'IngressController ingress-nginx/ingress-nginx -> layerx-testnet-public.layerx-testnet.svc:https [Ingress layerx-testnet/layerx-testnet-public testnet.layerx.network/]',
    'IngressController ingress-nginx/ingress-nginx -> layerx-gateway.layerx-testnet.svc:https [Ingress layerx-testnet/layerx-gateway api.testnet.layerx.network/]',
    'IngressController ingress-nginx/ingress-nginx -> layerx-dashboard-api.layerx-developer.svc:https [Ingress layerx-developer/layerx-developer developers.layerx.example/v1/dashboard]',
    'IngressController ingress-nginx/ingress-nginx -> layerx-webhooks.layerx-developer.svc:https [Ingress layerx-developer/layerx-developer developers.layerx.example/v1/webhooks]',
    'IngressController ingress-nginx/ingress-nginx -> layerx-dashboard-web.layerx-developer.svc:http [Ingress layerx-developer/layerx-developer-web developers.layerx.example/]',
)

HUMAN_EDGES = (
    'Deployment layerx-internal/journeys -> layerx-human.layerx-testnet.svc:9443 [env LAYERX_EVENTS_UPSTREAM_URL]',
    'Deployment layerx-internal/approvals -> layerx-human.layerx-testnet.svc:9443 [env LAYERX_EVENTS_UPSTREAM_URL]',
)

SEPARATELY_OPERATED_EDGES = (
    'CronJob layerx-testnet/layerx-testnet-status-publisher -> status-publisher.layerx-status.svc.cluster.local:443 [env LAYERX_STATUS_PUBLISH_URL]',
)

COMPLETE_EDGES = RESOLVED_EDGES + HUMAN_EDGES


def load(parser):
    topology = module['Topology']()
    for name in paths:
        path = (ROOT / 'platform/hosted') / name / 'deployment.yaml'
        for document in module[parser](path.read_text()):
            topology.add(document, str(path), 'layerx-developer' if name == 'webhooks' else 'default')
    return topology

def failures(topology):
    return [row for row in module['check'](topology) if row[0] == 'FAIL']

def labelled(rows, status):
    return [row[1] for row in rows if row[0] == status]

def named(rows, status, expected, description):
    observed = labelled(rows, status)
    assert len(observed) == len(set(observed)), (description, 'duplicate edge', observed)
    assert sorted(observed) == sorted(expected), (
        description,
        'unnamed: %s' % [edge for edge in observed if edge not in expected],
        'named but absent: %s' % [edge for edge in expected if edge not in observed],
    )
    assert len(observed) == len(expected), (description, len(observed), len(expected))

base = load('load_pyyaml')
baseline = module['check'](base)
named(baseline, 'FAIL', HUMAN_EDGES, 'baseline Human edges without the Human manifest')
assert all('layerx-human' in row[2] for row in failures(base))
named(baseline, 'ok', RESOLVED_EDGES, 'baseline resolved edges')
named(baseline, 'external', SEPARATELY_OPERATED_EDGES, 'baseline separately operated edges')
parity_failed = False
try:
    assert baseline == module['check'](load('load_builtin'))
except Exception as error:
    parity_failed = True
    print('FAIL full-manifest parser parity:', error)
print('PASS baseline without the Human manifest resolves the %d named edges and leaves exactly the %d named Human edges unresolved' % (len(RESOLVED_EDGES), len(HUMAN_EDGES)))

for parser in ('load_pyyaml', 'load_builtin'):
    for change, expected in [
        ('cycle', 'ExternalName cycle'),
        ('missing', 'ExternalName target Service'),
        ('offcluster', 'has no declared in-cluster Service'),
        ('port', 'exposes 444'),
        ('selector', 'selects no workload'),
        ('target', 'targetPort missing'),
        ('ingress', 'ingress NetworkPolicy'),
        ('egress', 'egress NetworkPolicy'),
        ('dns', 'does not admit DNS'),
    ]:
        topology = copy.deepcopy(load(parser))
        alias = topology.services[('layerx-internal', 'identity')]
        target = topology.services[('layerx-testnet', 'internal-identity')]
        if change == 'cycle':
            alias['externalName'] = 'identity.layerx-internal.svc'
        elif change == 'missing':
            del topology.services[('layerx-testnet', 'internal-identity')]
        elif change == 'offcluster':
            alias['externalName'] = 'absent.example'
        elif change == 'port':
            target['ports'][0]['port'] = '444'
        elif change == 'selector':
            target['selector'] = {'app': 'absent'}
        elif change == 'target':
            target['ports'][0]['targetPort'] = 'missing'
        elif change == 'ingress':
            policy = next(p for p in topology.policies if p['name'] == 'layerx-node-ingress')
            policy['ingress'] = []
        elif change == 'egress':
            policy = next(p for p in topology.policies if p['name'] == 'layerx-webhooks')
            policy['egress'] = []
        else:
            policy = next(p for p in topology.policies if p['name'] == 'payments')
            policy['egress'][0]['to'][0]['podSelector']['matchLabels']['k8s-app'] = 'not-dns'
        result = failures(topology)
        assert len(result) > len(HUMAN_EDGES) and any(expected in row[2] for row in result), (change, result)
        print('PASS refusal:', parser, change)

topology = module['Topology']()
for document in module['load_pyyaml']((ROOT / 'platform/hosted/node/deployment.yaml').read_text()):
    topology.add(document, 'node/deployment.yaml', 'incorrect-default')
assert all(workload['ns'] == 'layerx-testnet' for workload in topology.workloads)
assert all(workload['ns'] == 'layerx-developer' for workload in base.workloads if workload['source'].endswith('webhooks/deployment.yaml'))
print('PASS explicit namespace precedence and kubectl apply fallback namespace')

paths.append('human')
complete = load('load_pyyaml')
assert not failures(complete), failures(complete)
assert module['check'](complete) == module['check'](load('load_builtin'))
named(module['check'](complete), 'ok', COMPLETE_EDGES, 'complete resolved edges')
named(module['check'](complete), 'external', SEPARATELY_OPERATED_EDGES, 'complete separately operated edges')
print('PASS complete topology including Human resolves the %d named edges with both parsers' % len(COMPLETE_EDGES))

node = (ROOT / 'platform/hosted/node/deployment.yaml').read_text()
for parser in ('load_pyyaml', 'load_builtin'):
    documents = module[parser](node)
    stateful = next(doc for doc in documents if doc['kind'] == 'StatefulSet')
    containers = stateful['spec']['template']['spec']['containers']
    relay = next(container for container in containers if container['name'] == 'paxeer-relay')
    parsed = relay['args']
    if parser == 'load_pyyaml':
        expected_args = parsed
    else:
        assert parsed == expected_args, (parsed, expected_args)
    assert 'exec socat' in parsed[0]
    assert relay['env'][0]['name'] == 'LAYERX_NODE_PAXEER_RELAY_PORT'
print('PASS node sequence literal block consumes body and preserves following env')

raise SystemExit(1 if parity_failed else 0)
