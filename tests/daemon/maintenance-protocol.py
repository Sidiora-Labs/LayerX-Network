import json
from pathlib import Path
import struct
import subprocess
import sys

maintained = json.loads(Path('platform/hosted/gateway/tests/fixtures/maintained-authority.json').read_text())
historical = json.loads(Path('platform/hosted/authority/tests/fixtures/real-program-deploy-receipt.json').read_text())
for fixture in (historical, maintained):
    if fixture is maintained:
        expected = fixture['authority']['batch_evidence']
        attachment = expected['batch_identity']['receipt_hex']
        key = fixture['sequencer_public_key']
    else:
        assert fixture['proof_index'] == 0 and fixture['proof_count'] == 1 and fixture['proof_siblings'] == []
        expected = dict(header_hex=fixture['header_hex'], header_signature=fixture['header_signature_hex'],
                        receipt_proof_hex=struct.pack('>HHIIBI', 1, 0x4d50, 0, 1, 0, 0).hex())
        attachment = ''
        key = fixture['sequencer_public_key_hex']
    inputs = [fixture['receipt_hex'], attachment, expected['header_hex'], expected['header_signature'], key]
    result = subprocess.run([sys.argv[1]], input='\n'.join(inputs) + '\n', text=True, capture_output=True)
    assert result.returncode == 0, result.stderr
    batch_text, head_text = result.stdout.splitlines()
    batch = json.loads(batch_text)
    assert batch == dict(sequencer_public_key=key, batch_evidence=expected)
    if fixture is historical:
        assert batch_text == json.dumps(dict(sequencer_public_key=key, batch_evidence=expected), separators=(',', ':'))
    head = json.loads(head_text)
    assert head['current'] is True
    assert head['receipt_hex'] == (attachment or fixture['receipt_hex'])
    if attachment:
        assert head['state_root'] == attachment[-64:]
        assert head['batch_evidence']['batch_identity'] == expected['batch_identity']
        assert head['batch_evidence']['receipt_proof_hex'] == expected['batch_identity']['receipt_proof_hex']
    assert head['batch_evidence']['header_hex'] == expected['header_hex']
    assert head['batch_evidence']['header_signature'] == expected['header_signature']
print('native historical bytes, authenticated maintenance attachment, maintained head, stale sequence and tamper refusals passed')
