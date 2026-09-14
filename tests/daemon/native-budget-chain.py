import json
import os
from pathlib import Path
import runpy
import signal
import stat
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tests/bridge'))
from custody_chain import Chain

PUBLICATION = runpy.run_path(str(ROOT / 'tests/daemon/guarantor-publication-chain.py'))
CODEC = PUBLICATION['p']
SETTLEMENT = PUBLICATION['s']
MAX_REQUESTS = 16
MAX_BATCH = 128


def client_directory(path):
    path.mkdir(mode=0o700)
    os.chown(path, 4021, 4021)


def client_json(path, value):
    CODEC.atomic_json(path, value)
    os.chown(path, 4021, 4021)
    path.chmod(0o600)


def unique_fields(pairs):
    value = {}
    for key, item in pairs:
        assert key not in value, 'duplicate Budget fixture request field'
        value[key] = item
    return value


def read_request(path):
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, 'rb') as source:
        info = os.fstat(source.fileno())
        assert stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and info.st_uid == 4021
        assert 0 < info.st_size <= 4096, 'Budget fixture request bound'
        encoded = source.read(4097)
        assert len(encoded) <= 4096
    value = json.loads(encoded, object_pairs_hook=unique_fields)
    assert set(value) == {'version', 'operation', 'batch'}
    assert type(value['version']) is int and value['version'] == 1
    assert value['operation'] == 'finalize'
    assert type(value['batch']) is int and 1 <= value['batch'] <= MAX_BATCH
    return value


def authority(native, control, export):
    fields = dict(line.split('=', 1) for line in
                  (native / 'guarantor-1/identity/producer.env').read_text().splitlines())
    public = bytes.fromhex(fields['LAYERX_NODE_SEQUENCER_PUBLIC_KEY'])
    identity = bytes.fromhex(fields['LAYERX_NODE_SEQUENCER_ID'])
    first = int(fields['LAYERX_NODE_FIRST_BATCH'])
    last = int(fields['LAYERX_NODE_LAST_BATCH'])
    header = PUBLICATION['decode_header'](export['canonical_header'])
    assert len(public) == len(identity) == 32 and any(public) and any(identity)
    assert header[0] == 3 and header[1] == 77 and header[14] == identity
    assert first <= header[3] <= last and first > 0
    path = control / 'authority.csv'
    with path.open('x') as output:
        output.write('layerx-sequencer-authority-v1\n')
        output.write(f'{identity.hex()},{public.hex()},{header[2]},{first},{last},active\n')
    os.chown(path, 4021, 4021)
    path.chmod(0o600)


def finalize(native, private, control, build, chain_config, url, submitter, state, inputs, number, batch):
    evidence = private / f'request-{number}'
    evidence.mkdir(mode=0o700)
    export = PUBLICATION['replay'](native, evidence, batch, build)
    header = PUBLICATION['decode_header'](export['canonical_header'])
    assert header[0] == 3 and header[1] == 77 and header[3] == batch
    request = PUBLICATION['certificate'](export, chain_config, url, submitter, state, inputs)
    PUBLICATION['authorize'](request, inputs, chain_config['vault'])
    result = PUBLICATION['publish'](request, evidence, 'checkpoint')
    assert result['checkpoint_id'] == request['checkpoint_id']
    assert result['publication']['checkpoint_id'] == request['checkpoint_id']
    public = control / 'proofs' / str(number)
    client_directory(public)
    client_json(public / 'request.json', request)
    client_json(public / 'result.json', result)
    client_json(public / 'export.json', export)
    client_json(control / 'responses' / f'{number}.json', {
        'version': 1, 'batch': batch, 'checkpoint_id': request['checkpoint_id'],
        'request': str(public / 'request.json'), 'result': str(public / 'result.json'),
        'export': str(public / 'export.json'),
    })


def drive(work, environment, url):
    scenario = environment['LAYERX_TEST_NATIVE_BUDGET_SCENARIO']
    assert scenario in ('recovery', 'unknown', 'refusals')
    build = Path(environment['LAYERX_TEST_CUSTODY_BUILD_DIR'])
    control = work / 'budget-control'
    client_directory(control)
    for name in ('requests', 'responses', 'proofs'):
        client_directory(control / name)
    private = work / 'budget-publication'
    private.mkdir(mode=0o700)
    state, inputs = private / 'state', private / 'inputs'
    state.mkdir(mode=0o700)
    inputs.mkdir(mode=0o700)
    submitter = SETTLEMENT.Account.create()
    submitter_path = private / 'submitter.key'
    descriptor = os.open(submitter_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, 'w') as output:
        output.write('0x' + bytes(submitter.key).hex())
    chain = Chain(environment['LAYERX_TEST_CUSTODY_CHAIN_FILE'])
    assert chain.url == url
    chain.transaction('0x', submitter.address, value=10 ** 21)
    environment = environment | {
        'LAYERX_TEST_NATIVE_BUDGET_WORK': str(control),
        'LAYERX_TEST_NATIVE_BUDGET_PUBLICATION': '1',
        'LAYERX_TEST_PUBLICATION_CUSTODY_FILE': str(work / 'custody.json'),
    }
    with (work / 'native-budget.log').open('wb') as log:
        process = subprocess.Popen(['bash', 'tests/daemon/program-admission.sh', str(build), '--native-budget'],
                                   cwd=ROOT, env=environment, stdout=log, stderr=log, start_new_session=True)
        try:
            deadline = time.monotonic() + 90
            while not (control / 'funded.json').exists():
                assert process.poll() is None, 'native Budget fixture exited before actual funding'
                assert time.monotonic() < deadline, 'native Budget funding readiness deadline'
                time.sleep(.1)
            native = Path(json.loads((control / 'funded.json').read_text())['native'])
            assert native.parent == work and native.name.startswith('lxp-program-admission-')
            bootstrap = private / 'bootstrap'
            bootstrap.mkdir(mode=0o700)
            export = PUBLICATION['replay'](native, bootstrap, 1, build)
            authority(native, control, export)
            chain_config = json.loads((native / 'publication-chain.json').read_text())
            client_json(control / 'ready.json', {'version': 1, 'funded_batch': 1})
            next_request, last_batch = 1, 1
            deadline = time.monotonic() + 900
            while process.poll() is None:
                assert time.monotonic() < deadline, 'native Budget scenario deadline'
                request_path = control / 'requests' / f'{next_request}.json'
                if request_path.exists():
                    assert next_request <= MAX_REQUESTS, 'native Budget request count bound'
                    request = read_request(request_path)
                    assert request['batch'] > last_batch, 'Budget checkpoint must advance the actual native batch'
                    finalize(native, private, control, build, chain_config, url, submitter_path,
                             state, inputs, next_request, request['batch'])
                    last_batch = request['batch']
                    next_request += 1
                else:
                    time.sleep(.05)
            assert process.wait() == 0, 'real native Budget client refused'
            assert next_request > 1, 'native Budget client did not finalize a checkpoint'
        finally:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait(timeout=5)


def funded(native, control):
    CODEC.atomic_json(control / 'funded.json', {'native': str(native)})
    deadline = time.monotonic() + 180
    while not (control / 'ready.json').exists():
        assert time.monotonic() < deadline, 'native Budget authority readiness deadline'
        time.sleep(.1)
    assert (control / 'authority.csv').is_file()


if __name__ == '__main__':
    assert len(sys.argv) == 4 and sys.argv[1] == 'funded'
    funded(Path(sys.argv[2]), Path(sys.argv[3]))
