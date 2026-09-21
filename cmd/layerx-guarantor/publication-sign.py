#!/usr/bin/env python3
# Signs a producer's <checkpoint-id>.publication-request.json into the <checkpoint-id>.json
# authorization the producer verifies before it publishes settlement evidence.
#
# Usage:
#   publication-sign.py REQUEST.json OUTPUT_DIR [--owner KEY_FILE=0xRECIPIENT]...
#                       [--checkpoint-authority-key KEY_FILE] [--custody-reference 0xHEX64]
#                       [--merge SIGNED.json]... [--partial]
#
# Every key stays with the party that holds it: an account owner signs the recipient binding of its
# own balances, the checkpoint authority signs the deposit registration, and each of them may run
# this tool on its own machine. --partial writes <checkpoint-id>.partial.json holding only the
# signatures made so far; the next signer passes it back with --merge. Without --partial the tool
# writes <checkpoint-id>.json, and only when every signature the checkpoint needs is present and
# verifies. A key file is a PEM private key or 64 hexadecimal characters of Ed25519 seed, mode 0600.
import importlib.util
import json
import os
from pathlib import Path
import re
import stat
import sys
from types import SimpleNamespace

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

HERE = Path(__file__).resolve().parent


def load(name):
    spec = importlib.util.spec_from_file_location('publication_sign_' + name, HERE / (name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def require(value, message):
    if not value:
        raise ValueError(message)


def private_key(path):
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, 'rb') as source:
        info = os.fstat(source.fileno())
        require(stat.S_ISREG(info.st_mode) and stat.S_IMODE(info.st_mode) == 0o600
                and info.st_uid == os.geteuid() and 0 < info.st_size <= 4096,
                'signing key file is not a private 0600 file of this user: ' + str(path))
        data = source.read(4097)
    if data.lstrip().startswith(b'-----BEGIN'):
        key = serialization.load_pem_private_key(data, None)
        require(isinstance(key, Ed25519PrivateKey), 'signing key is not Ed25519: ' + str(path))
        return key
    text = data.decode('ascii').strip()
    require(re.fullmatch('(0x)?[0-9a-fA-F]{64}', text), 'signing key file format: ' + str(path))
    return Ed25519PrivateKey.from_private_bytes(bytes.fromhex(text[-64:]))


def public_bytes(key):
    return key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)


def arguments(values):
    require(len(values) >= 2 and not values[0].startswith('--') and not values[1].startswith('--'),
            'usage: publication-sign.py REQUEST.json OUTPUT_DIR [--owner KEY_FILE=0xRECIPIENT]... '
            '[--checkpoint-authority-key KEY_FILE] [--custody-reference 0xHEX64] [--merge SIGNED.json]... [--partial]')
    result = SimpleNamespace(request=Path(values[0]), output=Path(values[1]), owners=[], authority=None,
                             reference=None, merge=[], partial=False)
    index = 2
    while index < len(values):
        name = values[index]
        if name == '--partial':
            result.partial = True
            index += 1
            continue
        require(name in ('--owner', '--checkpoint-authority-key', '--custody-reference', '--merge')
                and index + 1 < len(values), 'publication signing option invalid: ' + name)
        value = values[index + 1]
        if name == '--owner':
            path, separator, recipient = value.rpartition('=')
            require(separator and path and re.fullmatch('0x[0-9a-fA-F]{40}', recipient)
                    and int(recipient, 16) != 0, '--owner takes KEY_FILE=0xRECIPIENT')
            result.owners.append((Path(path), bytes.fromhex(recipient[2:])))
        elif name == '--checkpoint-authority-key':
            require(result.authority is None, 'one checkpoint authority key')
            result.authority = Path(value)
        elif name == '--custody-reference':
            require(result.reference is None and re.fullmatch('0x[0-9a-fA-F]{64}', value)
                    and int(value, 16) != 0, '--custody-reference takes 0xHEX64')
            result.reference = bytes.fromhex(value[2:])
        else:
            result.merge.append(Path(value))
        index += 2
    return result


def sign(values):
    options = arguments(values)
    api, publication, authorization = load('settlement'), load('publication'), load('authorization')
    codec = SimpleNamespace(**vars(publication))
    request = json.loads(options.request.read_text())
    header = api.values(api.HEADER_TYPES, request['header'])
    digest = publication.raw(request['checkpoint_id'], 32)
    proof = publication.raw(request['validity_proof'])
    require(len(proof) <= 1_048_576 and api.checkpoint_hash(header, proof) == digest,
            'publication request checkpoint hash mismatch')
    balances, _, deposits, profile = publication.native_request(api, request, header, digest)
    require(balances, 'publication request carries no owner balance to authorize')
    bindings, deposit = {}, None
    for path in options.merge:
        earlier = publication.read_authorizations(path)
        require(type(earlier) is dict and earlier.get('version') == 2
                and publication.raw(earlier['checkpoint_id'], 32) == digest,
                'merged authorization is for another checkpoint: ' + str(path))
        for item in earlier['recipient_bindings']:
            key = publication.raw(item['account'], 32), publication.raw(item['asset'], 32)
            require(key not in bindings, 'merged recipient binding repeated: ' + str(path))
            bindings[key] = item
        if earlier.get('deposit_registration') is not None:
            require(deposit is None, 'merged deposit registration repeated: ' + str(path))
            deposit = earlier['deposit_registration']
    owners = {}
    for path, recipient in options.owners:
        key = private_key(path)
        require(public_bytes(key) not in owners, 'owner key repeated: ' + str(path))
        owners[public_bytes(key)] = key, recipient, path
    used = set()
    for fact in balances:
        if fact['authority'] not in owners:
            continue
        key, recipient, _ = owners[fact['authority']]
        require((fact['account'], fact['asset']) not in bindings, 'balance already carries a merged binding')
        message = (b'LX:SETTLE:RECIPIENT:v1\0' + header[1].to_bytes(4, 'big') + fact['account']
                   + fact['asset'] + recipient + digest)
        bindings[fact['account'], fact['asset']] = dict(
            account=publication.hx(fact['account']), asset=publication.hx(fact['asset']),
            recipient=publication.hx(recipient), request_anchor=publication.hx(digest),
            signature=publication.hx(key.sign(message)))
        used.add(fact['authority'])
    for public, (_, _, path) in owners.items():
        require(public in used, 'owner key holds no balance in this checkpoint: ' + str(path))
    if options.authority is not None:
        require(deposits, 'checkpoint replays no deposit for the checkpoint authority to register')
        require(deposit is None, 'deposit registration already merged')
        vault = profile[13:33]
        reference = options.reference if options.reference is not None else bytes(12) + vault
        message = authorization.deposit_message(codec, deposits, header, digest, reference)
        deposit = dict(vault=publication.hx(vault), custody_reference=publication.hx(reference),
                       signature=publication.hx(private_key(options.authority).sign(message)))
    else:
        require(options.reference is None, '--custody-reference needs --checkpoint-authority-key')
    known = {(fact['account'], fact['asset']) for fact in balances}
    require(set(bindings) <= known, 'recipient binding for a balance outside this checkpoint')
    missing = [publication.hx(fact['account']) for fact in balances
               if (fact['account'], fact['asset']) not in bindings]
    if deposits and deposit is None:
        missing.append('deposit registration')
    value = dict(version=2, checkpoint_id=publication.hx(digest),
                 recipient_bindings=[bindings[key] for key in sorted(bindings)], deposit_registration=deposit)
    # The producer's own checks, over exactly what is about to be written.
    publication.verified_bindings(value, [fact for fact in balances if (fact['account'], fact['asset']) in bindings],
                                  header, digest)
    if deposit is not None or not missing:
        message, signed, _, _, _ = publication.verified_deposit(value, deposits, profile, header, digest)
        if options.authority is not None:
            publication.signature(public_bytes(private_key(options.authority)), message, signed)
    require(not missing or options.partial, 'authorization is incomplete, still unsigned: ' + ', '.join(missing)
            + ' (pass --partial to hand it to the next signer)')
    require(options.output.is_dir(), 'output directory absent: ' + str(options.output))
    destination = options.output / (digest.hex() + ('.partial.json' if missing else '.json'))
    require(not destination.exists() and not destination.is_symlink(),
            'authorization output already exists: ' + str(destination))
    publication.atomic_json(destination, value)
    return destination, missing


if __name__ == '__main__':
    try:
        written, unsigned = sign(sys.argv[1:])
    except (OSError, ValueError, KeyError, TypeError) as error:
        raise SystemExit('publication signing refused: ' + str(error)) from None
    print(str(written) + (' (partial; still unsigned: ' + ', '.join(unsigned) + ')' if unsigned else ''))
