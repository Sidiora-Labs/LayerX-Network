import copy
import json
from pathlib import Path
import sys
import yaml

from provision import protected_json, require, write_json


def install(pod, field, item):
    existing = [value for value in pod.setdefault(field, []) if value['name'] == item['name']]
    require(not existing or existing == [item], item['name'], 'unchanged mounted runtime definition')
    if not existing:
        pod[field].append(item)


def configure(current, source, image, network, chain, tenant, funding, native_asset):
    result = copy.deepcopy(current)
    pod = result['spec']['template']['spec']
    canonical = source['spec']['template']['spec']
    require(not any(item['name'] in ('components', 'human-owner', 'human-onboarding')
                    for item in pod['containers']), 'onboarding', 'exclusive bootstrap store owner')
    for name in ('human-identity', 'human-kms'):
        item = copy.deepcopy(next(item for item in canonical['containers'] if item['name'] == name))
        item['image'] = image
        if name == 'human-identity':
            values = dict(LAYERX_HUMAN_IDENTITY_PROVIDER_BINDING_SOCKET='/run/layerx/human/identity-binding.sock',
                LAYERX_HUMAN_IDENTITY_PROVIDER_BINDING_TENANT=tenant,
                LAYERX_HUMAN_IDENTITY_PROVIDER_BINDING_ALLOWED_UIDS='4020,4021')
            item['env'] = [value for value in item['env'] if value['name'] not in values]
            item['env'].extend(dict(name=name, value=value) for name, value in values.items())
        install(pod, 'containers', item)
    component = next(item for item in canonical['containers'] if item['name'] == 'components')
    item = dict(name='human-onboarding', image=image, imagePullPolicy=component['imagePullPolicy'],
        command=['/usr/local/bin/human-entrypoint', 'onboarding-bootstrap'],
        args=['--network-id', str(network), '--chain-id', str(chain), '--tenant', tenant,
              '--initial-funding', str(funding), '--native-asset', native_asset],
        securityContext=copy.deepcopy(component['securityContext']),
        resources=copy.deepcopy(component['resources']),
        volumeMounts=[copy.deepcopy(mount) for mount in component['volumeMounts']
                      if mount['name'] != 'human-events'])
    item['volumeMounts'].append(dict(name='human-onboarding-input', mountPath='/run/onboarding-input', readOnly=True))
    item['readinessProbe'] = dict(exec=dict(command=['test', '-S', '/run/layerx/human/onboarding-signer.sock']),
                                  timeoutSeconds=1, periodSeconds=2)
    install(pod, 'containers', item)
    init = copy.deepcopy(next(item for item in canonical['initContainers'] if item['name'] == 'human-directories'))
    init['image'] = image
    install(pod, 'initContainers', init)
    needed = {mount['name'] for item in [init, *pod['containers']]
              for mount in item.get('volumeMounts', [])}
    for volume in canonical['volumes']:
        if volume['name'] in needed:
            install(pod, 'volumes', copy.deepcopy(volume))
    install(pod, 'volumes', dict(name='human-onboarding-input',
        secret=dict(secretName='layerx-human-onboarding-input', defaultMode=0o440)))
    result.pop('status', None)
    for field in ('managedFields', 'resourceVersion', 'uid', 'creationTimestamp', 'generation'):
        result['metadata'].pop(field, None)
    return result


if __name__ == '__main__':
    current, source, destination, image, network, chain, tenant, funding, native_asset = sys.argv[1:]
    documents = list(yaml.safe_load_all(Path(source).read_text()))
    source = next(document for document in documents if document['kind'] == 'StatefulSet')
    write_json(Path(destination), configure(protected_json(current), source, image,
        int(network), int(chain), tenant, int(funding), native_asset))
