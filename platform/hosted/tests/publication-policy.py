import json
import os
from pathlib import Path
import re
import sys


def hexadecimal(value, length, prefix=False):
    pattern = ('0x' if prefix else '') + '[0-9a-fA-F]{' + str(length * 2) + '}'
    if not re.fullmatch(pattern, value) or int(value, 16) == 0:
        raise ValueError('publication policy hexadecimal field invalid')
    return (value[2:] if prefix else value).lower()


def integer(value, limit):
    if not value.isdecimal() or not 0 < int(value) < limit:
        raise ValueError('publication policy domain invalid')
    return int(value)


def identity(value):
    if not value.isdecimal() or not 0 <= int(value) < 2**32:
        raise ValueError('publication policy peer identity invalid')
    return int(value)


def absolute(value):
    path = Path(value)
    if not path.is_absolute() or path.parent.resolve() != path.parent or path.name in ('', '.', '..'):
        raise ValueError('publication policy path invalid')
    return str(path)


# The cluster values of the signer sockets, their peer identities and the checkpoint authority key
# file. A guarantor run outside the cluster reaches the same signers over its own socket paths and
# keeps its checkpoint authority key where its operator put it, so each of these is an override
# `--name value` after the positional arguments rather than a constant.
DEFAULTS = {'treasury-socket': '/run/layerx/node/treasury-signer.sock',
            'human-socket': '/run/layerx/human/recipient.sock',
            'peer-uid': '4020',
            'peer-gid': '4020',
            'deposit-authority-key-file': '/var/lib/guarantor-submitter/checkpoint-authority.pem'}


def options(arguments):
    values, positional, index = dict(DEFAULTS), [], 0
    while index < len(arguments):
        item = arguments[index]
        if not item.startswith('--'):
            positional.append(item)
            index += 1
            continue
        name = item[2:]
        if name not in DEFAULTS or index + 1 == len(arguments):
            raise ValueError('publication policy option invalid')
        values[name] = arguments[index + 1]
        index += 2
    return positional, values


def main(arguments):
    arguments, option = options(arguments)
    if len(arguments) == 5 and arguments[0] == 'treasury':
        _, output, network, asset, recipient = arguments
        value = dict(version=1, network_id=integer(network, 2**32),
                     asset_id=hexadecimal(asset, 32), recipient=hexadecimal(recipient, 20))
    elif len(arguments) == 10 and arguments[0] == 'authorization':
        _, output, network, chain, bond, registry, vault, public, asset, recipient = arguments
        vault = hexadecimal(vault, 20, True)
        peers = dict(peer_uid=identity(option['peer-uid']), peer_gid=identity(option['peer-gid']))
        value = dict(version=1, network_id=integer(network, 2**32), chain_id=integer(chain, 2**64),
                     settlement_contract=hexadecimal(bond, 20, True),
                     checkpoint_registry=hexadecimal(registry, 20, True), vault=vault,
                     custody_reference='00' * 12 + vault,
                     treasury=dict(socket=absolute(option['treasury-socket']), **peers,
                                   public_key=hexadecimal(public, 32),
                                   asset_id=hexadecimal(asset, 32), recipient=hexadecimal(recipient, 20)),
                     human=dict(socket=absolute(option['human-socket']), **peers),
                     deposit_authority_key_file=absolute(option['deposit-authority-key-file']))
    else:
        raise ValueError('publication policy arguments invalid')
    path = Path(output)
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, 'w') as destination:
        json.dump(value, destination, sort_keys=True)
        destination.write('\n')
        destination.flush()
        os.fsync(destination.fileno())


if __name__ == '__main__':
    try:
        main(sys.argv[1:])
    except (OSError, ValueError):
        raise SystemExit('publication policy creation refused') from None
