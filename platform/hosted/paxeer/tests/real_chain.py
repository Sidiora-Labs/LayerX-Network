import http.client
import json
import os
from pathlib import Path
import socket
import shutil
import ssl
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[4]
PAXD = os.environ.get('PAXD', str(ROOT / 'paxeer-network/build/paxd'))


def port():
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        return listener.getsockname()[1]


def run(*args, **kwargs):
    return subprocess.run(args, check=True, stdout=subprocess.DEVNULL,
                          stderr=subprocess.PIPE, **kwargs)


with tempfile.TemporaryDirectory(prefix='layerx-paxeer-real-') as work:
    work = Path(work)
    env = os.environ.copy()
    env.update(PAXD=PAXD, LAYERX_PAXEER_HOME=str(work / 'chain'),
               LAYERX_PAXEER_CHAIN_ID='125',
               LAYERX_PAXEER_DEPLOYER_ADDRESS='0x7e5f4552091a69125d5dfcb7b8c2659029395bdf')
    for name in ['EVM', 'EVM_WS', 'RPC', 'P2P', 'GRPC', 'GRPC_WEB']:
        env['LAYERX_PAXEER_' + name + '_PORT'] = str(port())
    deployment_inputs = os.environ.get('LAYERX_PAXEER_TEST_DEPLOY_DIR')
    if deployment_inputs:
        env['LAYERX_PAXEER_DEPLOYER_ADDRESS'] = (Path(deployment_inputs)/'deployer.address').read_text().strip()
    processes = []
    try:
        run('bash', str(ROOT / 'platform/hosted/paxeer/init-chain.sh'), env=env)
        run('bash', str(ROOT / 'platform/hosted/paxeer/init-chain.sh'), env=env)
        with (work / 'node.log').open('w') as log:
            node = subprocess.Popen([PAXD, 'start', '--home', env['LAYERX_PAXEER_HOME'],
                                     '--log_level', env.get('LAYERX_PAXEER_TEST_LOG_LEVEL', 'info')],
                                    stdout=log, stderr=log, env=env)
        processes.append(node)
        run('openssl', 'req', '-x509', '-newkey', 'ec', '-pkeyopt', 'ec_paramgen_curve:P-256',
            '-nodes', '-keyout', str(work/'ca.key'), '-out', str(work/'ca.pem'), '-days', '1',
            '-subj', '/CN=LayerX beta test CA')
        run('openssl', 'req', '-new', '-newkey', 'ec', '-pkeyopt', 'ec_paramgen_curve:P-256',
            '-nodes', '-keyout', str(work/'key.pem'), '-out', str(work/'server.csr'), '-subj', '/CN=localhost')
        (work/'ext').write_text('subjectAltName=DNS:localhost,IP:127.0.0.1\nbasicConstraints=CA:FALSE\nextendedKeyUsage=serverAuth\n')
        run('openssl', 'x509', '-req', '-in', str(work/'server.csr'), '-CA', str(work/'ca.pem'),
            '-CAkey', str(work/'ca.key'), '-CAcreateserial', '-days', '1', '-extfile', str(work/'ext'),
            '-out', str(work/'cert.pem'))
        run('openssl', 'x509', '-in', str(work/'cert.pem'), '-outform', 'DER', '-out', str(work/'cert.der'))
        run('openssl', 'pkcs8', '-topk8', '-nocrypt', '-in', str(work/'key.pem'), '-outform', 'DER', '-out', str(work/'key.der'))
        boundary_port = port()
        env.update(LAYERX_PAXEER_BOUNDARY_LISTEN=f'127.0.0.1:{boundary_port}',
                   LAYERX_PAXEER_BOUNDARY_TLS_CERT_DER=str(work/'cert.der'),
                   LAYERX_PAXEER_BOUNDARY_TLS_KEY_DER=str(work/'key.der'),
                   LAYERX_PAXEER_NODE_URL='http://127.0.0.1:' + env['LAYERX_PAXEER_EVM_PORT'])
        with (work/'boundary.log').open('w') as log:
            boundary = subprocess.Popen([sys.argv[1]], env=env, stdout=log, stderr=log)
        processes.append(boundary)
        context = ssl.create_default_context(cafile=str(work/'ca.pem'))

        def request(method, path, body=None):
            connection = http.client.HTTPSConnection('localhost', boundary_port, context=context, timeout=5)
            connection.request(method, path, body, {'Content-Type': 'application/json'})
            response = connection.getresponse()
            data = response.read()
            assert response.getheader('Content-Type') == 'application/json'
            assert response.getheader('Cache-Control') == 'no-store'
            connection.close()
            return response.status, json.loads(data)

        deadline = time.monotonic() + 90
        while time.monotonic() < deadline:
            assert node.poll() is None, (work/'node.log').read_text()[-8000:]
            try:
                if request('GET', '/readyz')[0] == 200:
                    break
            except (OSError, http.client.HTTPException):
                pass
            time.sleep(.2)
        else:
            raise AssertionError('node failed readiness: ' + (work/'node.log').read_text()[-8000:])
        for identifier in [1, 'preserved', None]:
            status, data = request('POST', '/', json.dumps({'jsonrpc':'2.0', 'id':identifier, 'method':'eth_chainId', 'params':[]}))
            assert status == 200 and data['result'] == '0x7d' and data['id'] == identifier
        status, data = request('POST', '/', '{"jsonrpc":"2.0","id":"error","method":"eth_missingMethod","params":[]}')
        assert status == 200 and data['id'] == 'error' and 'error' in data
        payload = '{"jsonrpc":"2.0","id":"limit","method":"eth_chainId","params":[]}'
        payload += ' ' * (2 * 1024 * 1024 - len(payload))
        status, data = request('POST', '/', payload)
        assert status == 200 and data['result'] == '0x7d', data
        assert request('POST', '/', payload + ' ')[0] == 413
        for selector, expected_word in [('0x313ce567', 6), ('0x8da5cb5b', int(env['LAYERX_PAXEER_DEPLOYER_ADDRESS'], 16))]:
            status, data = request('POST', '/', json.dumps({'jsonrpc': '2.0', 'id': 'token', 'method': 'eth_call',
                'params': [{'to': '0x85FcD13735F4309833A503EE804ea32395851479', 'data': selector}, 'latest']}))
            assert status == 200 and int(data['result'], 16) == expected_word, data
        assert request('GET', '/other')[0] == 404
        assert request('PUT', '/')[0] == 404
        assert request('GET', '/livez')[0] == 200
        # Every listener for the actual EVM socket must be loopback.
        expected = f'{int(env["LAYERX_PAXEER_EVM_PORT"]):04X}'
        rows = [line.split()[1] for line in Path('/proc/net/tcp').read_text().splitlines()[1:]
                if line.split()[1].endswith(':' + expected) and line.split()[3] == '0A']
        assert rows == ['0100007F:' + expected], rows
        run('openssl', 'x509', '-in', str(work/'ca.pem'), '-outform', 'DER', '-out', str(work/'ca.der'))
        client_env = env | {'LAYERX_PAXEER_TEST_CLIENT_URL': f'https://localhost:{boundary_port}',
                            'LAYERX_PAXEER_TEST_CLIENT_CA': str(work/'ca.der')}
        run(sys.argv[2], '--exact', 'real_paxd_boundary', env=client_env)
        signing_key = ((Path(deployment_inputs)/'keys/deployer.key').read_text().strip()
                       if deployment_inputs else '0x' + '0' * 63 + '1')
        signed = subprocess.run(['cast', 'mktx', '--rpc-url', f'https://localhost:{boundary_port}',
            '--chain', '125', '--nonce', '0', '--gas-limit', '21000', '--gas-price', '3000000000',
            '--priority-gas-price', '1000000000', '--private-key', signing_key,
            '--value', '1', '0x2B5AD5c4795c026514f8317c7a215E218DcCD6cF'],
            env=env | {'SSL_CERT_FILE': str(work/'ca.pem')}, capture_output=True, text=True)
        assert signed.returncode == 0, 'signing real Paxeer probe transaction failed'
        status, data = request('POST', '/', json.dumps({'jsonrpc': '2.0', 'id': 'transfer',
            'method': 'eth_sendRawTransaction', 'params': [signed.stdout.strip()]}))
        assert status == 200 and 'result' in data, data
        transaction = data['result']
        for _ in range(100):
            status, receipt = request('POST', '/', json.dumps({'jsonrpc': '2.0', 'id': 'receipt',
                'method': 'eth_getTransactionReceipt', 'params': [transaction]}))
            if receipt.get('result') is not None:
                break
            time.sleep(.2)
        else:
            raise AssertionError(f'real EVM transfer receipt unavailable: {transaction}')
        assert status == 200 and receipt['result']['status'] == '0x1', receipt
        if deployment_inputs:
            inputs = Path(deployment_inputs)
            settlement = inputs/'checkpoint-settlement.json'
            settlement.write_bytes((ROOT/'contracts/config/checkpoint-settlement.json').read_bytes())
            deploy_env = client_env | {
                'LAYERX_PAXEER_BOUNDARY_URL': f'https://localhost:{boundary_port}',
                'LAYERX_PAXEER_BOUNDARY_CA_DER': str(work/'ca.der'),
                'LAYERX_PAXEER_DEPLOYER_KEY_FILE': str(inputs/'keys/deployer.key'),
                'LAYERX_PAXEER_GENESIS_DIR': str(inputs/'genesis'),
                'LAYERX_PAXEER_DEPLOYMENT_INPUT': str(inputs/'deployment-input.json'),
                'LAYERX_PAXEER_GUARANTORS': str(inputs/'guarantors.json'),
                'LAYERX_PAXEER_GUARANTOR_KEYS_DIR': str(inputs/'keys'),
                'LAYERX_PAXEER_DEPLOYMENT_RECORD': str(inputs/'deployment.json'),
                'LAYERX_PAXEER_SETTLEMENT_JSON': str(settlement)}
            immediate = json.loads((inputs/'deployment-input.json').read_text()).get('timelock_profile') == 'immediate-beta'
            phase = 'bootstrap' if immediate else 'deploy'
            try:
                run('bash', str(ROOT/'platform/hosted/paxeer/deploy-contracts.sh'), phase, env=deploy_env)
            except subprocess.CalledProcessError as error:
                sys.stderr.buffer.write(error.stderr)
                raise
            if immediate:
                deployment = json.loads((inputs/'deployment.json').read_text())
                assert deployment['phases'] == ['deploy', 'permissions', 'activate', 'bond', 'finalize'], deployment['phases']
                run('bash', str(ROOT/'platform/hosted/paxeer/deploy-contracts.sh'), 'status', env=deploy_env)
            domain = json.loads(settlement.read_text())['settlement_domains']['beta']
            assert domain['paxeer_chain_id'] == 125
            assert domain['guarantor_set'] == [{key: row[key].lower() for key in ['guarantor_id', 'signer', 'public_key']}
                                               for row in json.loads((inputs/'guarantors.json').read_text())]
        healthy_port = boundary_port
        boundary_port = port()
        wrong_env = env | {'LAYERX_PAXEER_CHAIN_ID': '126',
                           'LAYERX_PAXEER_BOUNDARY_LISTEN': f'127.0.0.1:{boundary_port}'}
        with (work/'wrong-chain.log').open('w') as log:
            wrong = subprocess.Popen([sys.argv[1]], env=wrong_env, stdout=log, stderr=log)
        processes.append(wrong)
        for _ in range(50):
            try:
                status, data = request('GET', '/readyz')
                break
            except OSError:
                time.sleep(.1)
        else:
            raise AssertionError('wrong-chain boundary did not start')
        assert status == 503 and data['error']['code'] == 'chain_id_mismatch', data
        boundary_port = healthy_port
        node.terminate()
        node.wait(timeout=20)
        assert request('GET', '/readyz')[0] == 503
        assert request('GET', '/livez')[0] == 200
        print('real paxd: chain 125, TLS relay, preserved IDs, RPC errors, loopback bind, node-loss readiness passed')
    except subprocess.CalledProcessError as error:
        sys.stderr.write(error.stderr.decode(errors='replace'))
        raise
    finally:
        if (work/'node.log').exists():
            shutil.copyfile(work/'node.log', '/tmp/paxeer-last-node.log')
        for process in reversed(processes):
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=20)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
