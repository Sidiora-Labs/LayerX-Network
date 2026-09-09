#!/usr/bin/env python3
import sys
from pathlib import Path

import yaml


def bootstrap(documents):
    for document in documents:
        if document['kind'] != 'StatefulSet':
            continue
        pod = document['spec']['template']['spec']
        pod['containers'] = [c for c in pod['containers']
                             if c['name'] not in {'human', 'components'}
                             and not c['name'].startswith('human-')]
        pod['initContainers'] = [c for c in pod['initContainers']
                                 if c['name'] != 'human-authority-material']
        authority = next(c for c in pod['containers'] if c['name'] == 'receipt-authority')
        human_env = {'LAYERX_AUTHORITY_HUMAN_AGENT_TOKEN_FILE',
                     'LAYERX_AUTHORITY_HUMAN_AGENT_TENANT',
                     'LAYERX_AUTHORITY_HUMAN_AGENT_PRINCIPAL',
                     'LAYERX_AUTHORITY_PRINCIPAL_POLICY_FILE',
                     'LAYERX_AUTHORITY_MODULE_REGISTRY_FILE',
                     'LAYERX_AUTHORITY_CORE_CLOCK_HORIZON',
                     'LAYERX_AUTHORITY_STATE_ROOT'}
        authority['env'] = [e for e in authority['env'] if e['name'] not in human_env]
        authority['volumeMounts'] = [v for v in authority['volumeMounts']
                                     if v['name'] != 'human-authority-private']
        used = {v['name'] for c in pod['containers'] + pod['initContainers']
                for v in c.get('volumeMounts', [])}
        pod['volumes'] = [v for v in pod['volumes'] if v['name'] in used]
    return documents


if __name__ == '__main__':
    documents = list(yaml.safe_load_all(Path(sys.argv[1]).read_text()))
    Path(sys.argv[2]).write_text(yaml.safe_dump_all(bootstrap(documents), sort_keys=False))
