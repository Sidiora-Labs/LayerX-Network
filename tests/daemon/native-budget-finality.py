import os
import stat
import struct
import subprocess
import sys


def registrations(settlement, root, build, request, result, export, first_batch, private, public):
    raw = settlement.raw
    header = raw(export['canonical_header'])
    assert len(header) == 354 and first_batch > 0
    assert request['checkpoint_id'] == result['checkpoint_id']
    assert result['paxeer_chain_id'] == request['chain_id']
    assert raw(result['settlement_contract'], 20) == raw(request['settlement_contract'], 20)
    proof = raw(request['validity_proof'])
    assert len(proof) <= 1048576
    encoded = bytearray(b'LXBFIN1\0')
    encoded += struct.pack('>Q', request['chain_id'])
    encoded += raw(request['settlement_contract'], 20) + raw(request['checkpoint_registry'], 20)
    encoded += struct.pack('>Q', result['set_version'])
    encoded += raw(result['checkpoint_id'], 32) + raw(result['transaction_id'], 32)
    encoded += struct.pack('>QQQ', result['observed_block_number'], result['observed_at_ms'], first_batch)
    encoded += header + struct.pack('>I', len(proof)) + proof
    encoded += struct.pack('>I', len(request['attestations']))
    for attestation in request['attestations']:
        fields = settlement.values(settlement.ATTESTATION_TYPES, attestation)
        signed = settlement.encode_packed(settlement.ATTESTATION_TYPES[:14], fields[:14])
        digest = settlement.hashlib.sha256(b'LXP/v2/guarantor-attestation\0' + signed).digest()
        signature = settlement.keys.Signature(vrs=(fields[17] - 27,
            int.from_bytes(fields[15], 'big'), int.from_bytes(fields[16], 'big')))
        key = signature.recover_public_key_from_msg_hash(digest)
        assert raw(key.to_checksum_address(), 20) == raw(fields[14], 20)
        record = settlement.encode_packed(settlement.ATTESTATION_TYPES, fields)
        assert len(record) == 274
        encoded += fields[7] + key.to_compressed_bytes() + record
    assert len(encoded) <= 2 * 1024 * 1024
    batch = request['header'][3]
    material = private / f'{batch}.material'
    descriptor = os.open(material, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, 'wb') as output:
        output.write(encoded)
        output.flush()
        os.fsync(output.fileno())
    checkpoint = public / f'{batch}.checkpoint'
    finality = public / f'{batch}.finality'
    with (private / f'{batch}.finality.log').open('wb') as log:
        subprocess.run([str(build / 'tests/lxp_test_native_budget_finality'), str(material),
            request['rpc_url'], request['submitter_key_file'], request['publication_state_dir'],
            sys.executable, str(root / 'cmd/layerx-guarantor/settlement.py'), str(checkpoint), str(finality)],
            cwd=root, stdout=log, stderr=log, check=True, timeout=90)
    for path in (checkpoint, finality):
        info = path.lstat()
        assert stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and 0 < info.st_size <= 2 * 1024 * 1024
        os.chown(path, 4021, 4021)
        path.chmod(0o600)
    return {'batch': batch, 'checkpoint_id': raw(result['checkpoint_id'], 32).hex(),
            'checkpoint': str(checkpoint), 'finality': str(finality)}
