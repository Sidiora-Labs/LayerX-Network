import base64
import json
import os
from pathlib import Path
import subprocess
import time
import urllib.request
import urllib.error

from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

from custody_credit import (DEPOSIT_TOPIC, MAX_ANCESTRY, MAX_RESPONSE, MAX_RPC_CALLS,
                            MAX_TOTAL_RESPONSE, PROFILE_BYTES, big, eth_hash, quantity,
                            read_key, require, sha, unhex, write_new)

ROOT = Path(__file__).resolve().parents[2]
VERSION = 'paxeer-custody-state-v2'
PROOF_KIND = 1


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, 'duplicate JSON field')
        result[key] = value
    return result


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=True).encode()


def comet(rpc, method, params):
    require(getattr(rpc, 'disposable', False), 'Comet requires verified disposable identity')
    deadline = time.monotonic()+30
    for attempt in range(6):
        if method == 'abci_query':
            delay = rpc.budget.get('next_proof', 0)-time.monotonic()
            if delay > 0:
                require(time.monotonic()+delay < deadline, 'Comet proof pacing deadline')
                time.sleep(delay)
        require(rpc.budget['calls'] < MAX_RPC_CALLS, 'aggregate RPC work bound')
        rpc.budget['calls'] += 1
        identifier = rpc.budget['calls']
        request = urllib.request.Request(rpc.url.rstrip('/') + '/comet', method='POST',
            data=canonical(dict(jsonrpc='2.0', id=identifier, method=method, params=params)),
            headers={'Content-Type': 'application/json'})
        remaining = min(MAX_RESPONSE, MAX_TOTAL_RESPONSE-rpc.budget['bytes'])
        require(remaining > 0, 'aggregate RPC response bound')
        timeout = deadline-time.monotonic()
        require(timeout > 0, 'Comet request deadline')
        try:
            response = rpc.opener.open(request, timeout=timeout)
        except urllib.error.HTTPError as refused:
            response = refused
        with response:
            data = response.read(remaining+1)
            status = response.status
            retry_after = response.headers.get('Retry-After')
        if method == 'abci_query':
            rpc.budget['next_proof'] = time.monotonic()+1.05
        rpc.budget['bytes'] += len(data)
        require(len(data) <= remaining, 'aggregate RPC response bound')
        value = json.loads(data, object_pairs_hook=unique_object)
        if status == 503:
            require(isinstance(value.get('error'), dict) and
                    value['error'].get('code') in ('comet_unavailable', 'comet_evidence_unavailable') and
                    value['error'].get('retry') == 'after',
                    'Comet typed temporary refusal')
            require(isinstance(retry_after, str) and retry_after.isascii()
                    and retry_after.isdecimal() and 0 < int(retry_after) <= 10,
                    'Comet retry delay bound')
            require(value['error'].get('retry_after_seconds') == int(retry_after), 'Comet retry metadata binding')
            delay = int(retry_after)
            require(attempt < 5 and time.monotonic()+delay < deadline, 'Comet temporary refusal deadline')
            time.sleep(delay)
            continue
        require(status == 200, 'Comet HTTP status: '+str(status))
        require(set(value) == {'jsonrpc', 'id', 'result'} and value['jsonrpc'] == '2.0'
                and type(value['id']) is int and value['id'] == identifier, 'Comet response envelope')
        return value['result']
    raise ValueError('Comet retry work bound')


def positive_decimal(value):
    require(isinstance(value, str) and value.isascii() and value.isdecimal()
            and value[0] != '0' and len(value) <= 19, 'Comet height encoding')
    result = int(value)
    require(0 < result < 2**63, 'Comet height bound')
    return result


def validators(rpc, height):
    pages = []
    total = None
    for page in range(1, 11):
        result = comet(rpc, 'validators', dict(height=str(height), page=str(page), per_page='100'))
        count = positive_decimal(result['count'])
        observed_total = positive_decimal(result['total'])
        require(positive_decimal(result['block_height']) == height and observed_total <= 1000
                and count == len(result['validators']) and count <= 100
                and (total is None or total == observed_total), 'validator page identity')
        total = observed_total
        require(count == min(100, total-(page-1)*100), 'validator page completeness')
        pages.append(result)
        if page*100 >= total:
            return pages
    raise ValueError('validator page bound')


def state_query(rpc, height, key):
    result = comet(rpc, 'abci_query', dict(path='/store/evm/key', data='0x'+key.hex(),
                                        height=str(height), prove=True))
    require(set(result) == {'response'}, 'ABCI response fields')
    return result['response']


def layout(args, runtime):
    artifact_path = Path(getattr(args, 'vault_artifact', None) or
        os.environ.get('LAYERX_CUSTODY_VAULT_ARTIFACT', ROOT/'build/forge-artifacts/LayerXVault.sol/LayerXVault.json'))
    encoded = artifact_path.read_bytes()
    require(len(encoded) <= 16*1024*1024, 'vault compiler artifact bound')
    artifact = json.loads(encoded, object_pairs_hook=unique_object)
    deployed = artifact['deployedBytecode']
    compiled = bytearray(unhex(deployed['object']))
    observed = bytearray(runtime)
    require(len(compiled) == len(observed) and len(compiled) > 0, 'vault compiler runtime length')
    ranges = set()
    for references in deployed['immutableReferences'].values():
        for reference in references:
            start, length = reference['start'], reference['length']
            require(type(start) is int and type(length) is int and length == 32
                    and 0 <= start <= len(compiled)-length, 'vault immutable reference bounds')
            indexes = set(range(start, start+length))
            require(not ranges.intersection(indexes), 'overlapping vault immutable references')
            ranges.update(indexes)
            require(compiled[start:start+length] == bytes(length), 'vault compiler immutable bytes')
            observed[start:start+length] = bytes(length)
    require(compiled == observed, 'vault compiler runtime binding')
    storage = artifact['storageLayout']
    matches = [entry for entry in storage['storage'] if entry['label'] == 'recordedDeposit']
    require(len(matches) == 1 and matches[0]['offset'] == 0, 'recordedDeposit compiler layout')
    entry = matches[0]
    mapping = storage['types'][entry['type']]
    require(mapping['encoding'] == 'mapping' and
            storage['types'][mapping['key']]['label'] == 'bytes32' and
            storage['types'][mapping['value']]['label'] == 'bool', 'recordedDeposit compiler type')
    slot = int(entry['slot'])
    require(0 <= slot < 2**256 and str(slot) == entry['slot'], 'recordedDeposit compiler slot')
    return big(slot, 32), sha(encoded)


def verifier(request):
    binary = Path(os.environ.get('LAYERX_CUSTODY_PROOF_BIN', ROOT/'build/bin/layerx-custody-proof'))
    encoded = canonical(request)
    require(0 < len(encoded) <= MAX_TOTAL_RESPONSE, 'Comet proof request bound')
    command = [str(binary)]
    state = os.environ.get('LAYERX_CUSTODY_HISTORY_STATE')
    authority = os.environ.get('LAYERX_CUSTODY_HISTORY_KEY')
    require(bool(state) == bool(authority), 'history state and authority configuration')
    if state:
        command += ['--history-state', state, '--attestor-key', authority]
    completed = subprocess.run(command, input=encoded, capture_output=True, timeout=120, check=False)
    require(completed.returncode == 0, 'Comet proof verification refused: ' +
            completed.stderr[:1024].decode('utf-8', errors='replace'))
    require(len(completed.stdout) <= (MAX_TOTAL_RESPONSE if request.get('operation') == 'export' else 2*1024*1024),
            'Comet verifier result bound')
    result = json.loads(completed.stdout, object_pairs_hook=unique_object)
    if request.get('operation') == 'export':
        require(isinstance(result, list) and 0 < len(result) <= MAX_ANCESTRY, 'history export bound')
    else:
        require(result['version'] == VERSION, 'Comet verifier version')
    return result


def history_state(args, genesis_hash):
    return Path(getattr(args, 'history_state', None) or Path(args.attestor_key).resolve().parent/'secrets'/('custody-history-'+genesis_hash.hex()))


def catch_up(rpcs, args, genesis_hash, final):
    from deploy_local_custody import genesis_document

    os.environ['LAYERX_CUSTODY_HISTORY_STATE'] = str(history_state(args, genesis_hash))
    os.environ['LAYERX_CUSTODY_HISTORY_KEY'] = str(Path(args.attestor_key).resolve())
    documents = [genesis_document(rpc, 'boundary') for rpc in rpcs]
    require(documents[0] == documents[1] and sha(documents[0]) == genesis_hash, 'Comet pinned genesis bytes')
    expected = dict(genesis_sha256='0x'+genesis_hash.hex(), comet_chain_id=rpcs[0].comet_chain_id, chain_id=125,
                    vault='', runtime_sha256='', confirmations=0)
    bundle = dict(version=VERSION, genesis=base64.b64encode(documents[0]).decode(), history=[], state_height=0,
                  finalized_height=0, state=[])
    request = dict(operation='status', expected=expected, bundle=bundle)
    status = verifier(request)
    require(status['height'] <= final+1, 'Comet endpoint head rollback')
    while status['height'] < final+1:
        start = status['height']+1
        end = min(start+127, final+1)
        budget = {'calls': 0, 'bytes': 0}
        for rpc in rpcs:
            rpc.budget = budget
        for rpc in rpcs:
            entries = []
            for height in range(start, end+1):
                commit = comet(rpc, 'commit', dict(height=str(height)))
                require(positive_decimal(commit['signed_header']['header']['height']) == height, 'Comet requested commit height')
                entries.append(dict(commit=commit, validators=validators(rpc, height)))
            advance = dict(operation='advance', expected=expected, bundle=bundle | {'history': entries})
            status = verifier(advance)
            require(status['height'] == end, 'authenticated history progress')
            if getattr(args, 'history_evidence', None):
                origin = sha(rpc.url.encode()).hex()
                write_new(str(Path(args.history_evidence)/f'{start}-{end}-{origin}.json'), canonical(advance)+b'\n')
    budget = {'calls': 0, 'bytes': 0}
    for rpc in rpcs:
        rpc.budget = budget
    return documents


def verified_state(rpcs, args, genesis_hash, vault, runtime_hash, confirmations, deposit_id=None, minimum_height=2):
    require(len(rpcs) == 2 and 0 < confirmations < MAX_ANCESTRY, 'Comet quorum and confirmation bound')
    deadline = time.monotonic()+30
    while True:
        tips = [comet(rpc, 'commit', {}) for rpc in rpcs]
        final = min(positive_decimal(tip['signed_header']['header']['height']) for tip in tips)-2
        height = final-confirmations+1
        if height >= minimum_height:
            break
        require(time.monotonic() < deadline, 'Comet deposit confirmation deadline')
        time.sleep(.1)
    require(2 <= height <= final < 2**63-1, 'Comet finalized history bound')
    documents = catch_up(rpcs, args, genesis_hash, final)
    points = sorted({height, final})
    requests, results = [], []
    compiler_hash = None
    for rpc, genesis in zip(rpcs, documents, strict=True):
        history = []
        for number in range(max(1, final-126), final+2):
            commit = comet(rpc, 'commit', dict(height=str(number)))
            require(positive_decimal(commit['signed_header']['header']['height']) == number,
                    'Comet requested commit height')
            history.append(dict(commit=commit, validators=validators(rpc, number)))
        state = []
        address = unhex(vault, 20)
        slot = None
        for number in points:
            code = state_query(rpc, number, b'\x07'+address)
            runtime = base64.b64decode(code['value'], validate=True)
            require(sha(runtime) == runtime_hash, 'vault runtime pin')
            selected_slot, selected_hash = layout(args, runtime)
            require((slot is None or slot == selected_slot) and
                    (compiler_hash is None or compiler_hash == selected_hash), 'vault compiler consistency')
            slot, compiler_hash = selected_slot, selected_hash
            point = dict(height=number, code=code, code_hash=state_query(rpc, number, b'\x08'+address))
            if deposit_id is not None:
                point['deposit'] = state_query(rpc, number, b'\x03'+address+eth_hash(deposit_id+slot))
            state.append(point)
        expected = dict(genesis_sha256='0x'+genesis_hash.hex(), comet_chain_id=rpc.comet_chain_id,
                        chain_id=125, vault=vault, runtime_sha256='0x'+runtime_hash.hex(), confirmations=confirmations)
        if deposit_id is not None:
            expected.update(deposit_id='0x'+deposit_id.hex(), deposit_slot='0x'+slot.hex())
        request = dict(operation='verify', expected=expected, bundle=dict(version=VERSION,
            genesis=base64.b64encode(genesis).decode(), history=history,
            state_height=height, finalized_height=final, state=state))
        result = verifier(request)
        require(result['state_height'] == height and result['finalized_height'] == final
                and unhex(result['runtime_sha256'], 32) == runtime_hash, 'verified state identity')
        requests.append(request)
        results.append(result)
    for field in ('state_header_hash', 'application_root', 'finalized_header_hash', 'runtime_sha256', 'runtime', 'deposit_id'):
        require(results[0].get(field) == results[1].get(field), 'verified Comet state quorum')
    evidence = dict(version=VERSION, requests=requests, results=results, compiler_artifact_sha256='0x'+compiler_hash.hex())
    encoded = canonical(evidence)+b'\n'
    result = results[0] | {'proof_sha256': '0x'+sha(encoded).hex()}
    return result, encoded


def create_profile(args, rpcs, genesis):
    require(args.chain_id == 125 and args.network_id > 0, 'Paxeer custody profile identity')
    result, evidence = verified_state(rpcs, args, genesis, args.vault,
        unhex(args.runtime_sha256, 32), args.confirmations)
    public = read_key(args.attestor_key).public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    name = b'system:paxeer-reserve'
    reserve = sha(b'LX:ACCOUNT:v1'+big(len(name), 4)+name)
    profile = (b'LXBC2'+big(125, 8)+unhex(args.vault, 20)+unhex(result['runtime_sha256'], 32)+public+
               unhex(args.asset, 32)+reserve+big(args.confirmations, 8)+genesis+big(args.network_id, 4)+big(3, 2))
    require(len(profile) == PROFILE_BYTES, 'Paxeer custody profile layout')
    write_new(args.output+'.proof.json', evidence)
    write_new(args.output, profile)


def attest(args, rpcs, genesis, profile):
    require(len(profile) == PROFILE_BYTES and profile[:5] == b'LXBC2'
            and int.from_bytes(profile[5:13], 'big') == 125 and profile[169:201] == genesis
            and args.network_id > 0 and profile[201:207] == big(args.network_id, 4)+big(3, 2),
            'Paxeer custody profile identity')
    transaction = '0x'+unhex(args.transaction, 32).hex()
    receipts = [rpc.call('eth_getTransactionReceipt', [transaction]) for rpc in rpcs]
    deposits = []
    for receipt in receipts:
        require(receipt is not None and unhex(receipt['transactionHash'], 32) == unhex(transaction, 32)
                and quantity(receipt['status']) == 1, 'custody deposit discovery receipt')
        logs = [value for value in receipt['logs'] if unhex(value['address'], 20) == profile[13:33]
                and value['topics'] and value['topics'][0].lower() == DEPOSIT_TOPIC]
        require(len(logs) == 1, 'exactly one custody deposit required')
        log = logs[0]
        require(log.get('removed') is False and len(log['topics']) == 4, 'custody deposit discovery log')
        deposit_id, asset, payer = [unhex(value, 32) for value in log['topics'][1:]]
        data = unhex(log['data'], 96)
        beneficiary, amount_word, nonce_word = data[:32], data[32:64], data[64:]
        amount, nonce = int.from_bytes(amount_word, 'big'), int.from_bytes(nonce_word, 'big')
        require(asset == profile[97:129] and payer[:12] == bytes(12) and
                0 < amount < 2**128 and 0 < nonce < 2**64 and amount == args.expected_amount
                and beneficiary == unhex(args.beneficiary, 32), 'custody deposit expected fields')
        domain = b'LXP/Paxeer/custody-deposit/v1'
        preimage = (big(256, 32)+big(125, 32)+bytes(12)+profile[13:33]+payer+asset+beneficiary+
                    amount_word+nonce_word+big(len(domain), 32)+domain.ljust(32, b'\0'))
        require(sha(preimage) == deposit_id, 'deposit ID preimage')
        deposits.append((deposit_id, asset, beneficiary, payer[12:], amount, nonce))
    require(deposits[0] == deposits[1], 'deposit discovery quorum')
    deposit_id, asset, beneficiary, payer, amount, nonce = deposits[0]
    owner = unhex(args.beneficiary_key, 32)
    minimum = max(quantity(receipt['blockNumber']) for receipt in receipts)
    result, evidence = verified_state(rpcs, args, genesis, '0x'+profile[13:33].hex(), profile[33:65],
                                     int.from_bytes(profile[161:169], 'big'), deposit_id, minimum)
    unsigned = (b'LXDC2'+sha(profile)+big(args.network_id, 4)+big(3, 2)+deposit_id+asset+beneficiary+
        owner+payer+big(amount, 16)+big(nonce, 8)+big(result['state_height'], 8)+
        unhex(result['state_header_hash'], 32)+unhex(result['application_root'], 32)+
        big(result['finalized_height'], 8)+unhex(result['finalized_header_hash'], 32)+
        unhex(result['proof_sha256'], 32)+big(PROOF_KIND, 4))
    require(len(unsigned) == 363, 'Paxeer custody credit layout')
    key = read_key(args.attestor_key)
    require(key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw) == profile[65:97], 'attestor authority')
    credit = unsigned+key.sign(b'LX:CUSTODY:CREDIT:v2'+unsigned)
    write_new(args.output+'.proof.json', evidence)
    write_new(args.output, credit)
    write_new(args.output+'.nullifier', sha(b'LX:DEPOSIT:NULLIFIER:v1'+deposit_id).hex().encode()+b'\n')
