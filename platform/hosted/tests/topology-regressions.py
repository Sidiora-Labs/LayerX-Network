import copy
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
script = (ROOT / 'platform/hosted/tests/topology-check.sh').read_text()
code = script.split("<<'PY'\n", 1)[1].split('\nPY\n', 1)[0]
module = {}
exec(compile(code.rsplit('sys.exit(main())', 1)[0], str(Path('platform/hosted/tests/topology-check.sh')), 'exec'), module)
paths = ['node', 'identity', 'paxeer', 'testnet', 'gateway', 'registry', 'internal', 'webhooks']

def load(parser):
    topology = module['Topology']()
    for name in paths:
        path = (ROOT / 'platform/hosted') / name / 'deployment.yaml'
        for document in module[parser](path.read_text()):
            topology.add(document, str(path), 'layerx-developer' if name == 'webhooks' else 'default')
    return topology

def failures(topology):
    return [row for row in module['check'](topology) if row[0] == 'FAIL']

base = load('load_pyyaml')
baseline = module['check'](base)
assert len(failures(base)) == 2
assert all('layerx-human' in row[2] for row in failures(base))
assert sum(row[0] == 'ok' for row in baseline) == 40
parity_failed = False
try:
    assert baseline == module['check'](load('load_builtin'))
except Exception as error:
    parity_failed = True
    print('FAIL full-manifest parser parity:', error)
print('PASS PyYAML resolves the real manifests; only two Human edges fail')

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
    topology = copy.deepcopy(base)
    alias = topology.services[('layerx-internal', 'component')]
    target = topology.services[('layerx-testnet', 'internal-component')]
    if change == 'cycle':
        alias['externalName'] = 'component.layerx-internal.svc'
    elif change == 'missing':
        del topology.services[('layerx-testnet', 'internal-component')]
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
    assert len(result) > 2 and any(expected in row[2] for row in result), (change, result)
    print('PASS refusal:', change)

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
assert sum(row[0] == 'ok' for row in module['check'](complete)) == 42
print('PASS complete topology including Human with both parsers')

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
