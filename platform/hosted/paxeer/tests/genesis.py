import base64
import hashlib
import http.client
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import socket
import ssl
import subprocess
import sys
import tempfile
import threading
import time
import unittest

FIXTURE = Path(__file__).parent / 'fixtures/pacific-1.genesis.json'
GENESIS = FIXTURE.read_bytes().strip()
LIMIT = 32 * 1024 * 1024
CHUNKED_ERROR = {'code': -32603, 'message': 'Internal error',
                 'data': 'genesis response is large, please use the genesis_chunked API instead'}


def encoded(value):
    return json.dumps(value, separators=(',', ':')).encode()


def rpc(value, error=False):
    return encoded({'jsonrpc': '2.0', 'id': -1, 'error' if error else 'result': value})


def direct(genesis=GENESIS, wrapped=False):
    result = b'{"genesis":' + genesis + b'}'
    return b'{"jsonrpc":"2.0","id":-1,"result":' + result + b'}' if wrapped else result


def chunk(data, index=0, total=1, wrapped=False):
    result = {'chunk': str(index), 'total': str(total),
              'data': base64.b64encode(data).decode()}
    return rpc(result) if wrapped else encoded(result)


class GenesisBoundary(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.work = tempfile.TemporaryDirectory(prefix='layerx-genesis-')
        work = Path(cls.work.name)
        cls.env = {key: value for key, value in os.environ.items()
                   if not key.startswith('LAYERX_PAXEER_')}
        commands = [
            ['req', '-x509', '-newkey', 'ec', '-pkeyopt', 'ec_paramgen_curve:P-256',
             '-nodes', '-keyout', 'key.pem', '-out', 'cert.pem', '-days', '1',
             '-subj', '/CN=localhost', '-addext', 'subjectAltName=DNS:localhost,IP:127.0.0.1'],
            ['x509', '-in', 'cert.pem', '-outform', 'DER', '-out', 'cert.der'],
            ['pkcs8', '-topk8', '-nocrypt', '-in', 'key.pem', '-outform', 'DER', '-out', 'key.der'],
        ]
        for args in commands:
            subprocess.run(['openssl'] + args, cwd=work, check=True, capture_output=True)
        cls.context = ssl.create_default_context(cafile=str(work/'cert.pem'))
        cls.env.update(LAYERX_PAXEER_CHAIN_ID='9125',
                       LAYERX_PAXEER_BOUNDARY_TLS_CERT_DER=str(work/'cert.der'),
                       LAYERX_PAXEER_BOUNDARY_TLS_KEY_DER=str(work/'key.der'))

    @classmethod
    def tearDownClass(cls):
        cls.work.cleanup()

    def setUp(self):
        self.calls = []
        self.responses = []
        case = self

        class Listener(BaseHTTPRequestHandler):
            protocol_version = 'HTTP/1.1'

            def log_message(self, *_):
                pass

            def do_GET(self):
                case.calls.append(self.path)
                if not case.responses:
                    self.send_error(500)
                    return
                expected, body, options = case.responses.pop(0)
                if self.path != expected:
                    self.send_error(400)
                    return
                time.sleep(options.get('delay', 0))
                try:
                    self.send_response(options.get('status', 200))
                    self.send_header('Content-Type', 'application/json')
                    self.send_header('Connection', 'close')
                    if options.get('http_chunked'):
                        self.send_header('Transfer-Encoding', 'chunked')
                        self.end_headers()
                        for offset in range(0, len(body), 65536):
                            part = body[offset:offset+65536]
                            self.wfile.write(f'{len(part):x}\r\n'.encode() + part + b'\r\n')
                        self.wfile.write(b'0\r\n\r\n')
                    else:
                        self.send_header('Content-Length', str(options.get('length', len(body))))
                        self.end_headers()
                        self.wfile.write(body)
                except (BrokenPipeError, ConnectionResetError):
                    pass
                self.close_connection = True

        self.server = ThreadingHTTPServer(('127.0.0.1', 0), Listener)
        self.thread = threading.Thread(target=self.server.serve_forever)
        self.thread.start()
        self.processes = []
        self.start_boundary(f'http://127.0.0.1:{self.server.server_port}')

    def tearDown(self):
        for process in self.processes:
            process.terminate()
            process.wait(timeout=5)
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()

    def start_boundary(self, comet):
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            self.port = listener.getsockname()[1]
        env = self.env | {'LAYERX_PAXEER_BOUNDARY_LISTEN': f'127.0.0.1:{self.port}',
                          'LAYERX_PAXEER_NODE_URL': f'http://127.0.0.1:{self.server.server_port}'}
        if comet is not None:
            env['LAYERX_PAXEER_COMET_URL'] = comet
        process = subprocess.Popen([sys.argv[1]], env=env, stdout=subprocess.DEVNULL,
                                   stderr=subprocess.DEVNULL)
        self.processes.append(process)
        for _ in range(100):
            self.assertIsNone(process.poll(), 'boundary exited during startup')
            try:
                if self.request('/livez')[0] == 200:
                    return
            except (OSError, http.client.HTTPException):
                time.sleep(.02)
        self.fail('boundary did not listen')

    def request(self, path='/genesis', method='GET', body=None):
        connection = http.client.HTTPSConnection('localhost', self.port, context=self.context, timeout=40)
        try:
            connection.request(method, path, body=body, headers={'Content-Type': 'application/json'})
            response = connection.getresponse()
            return response.status, dict(response.getheaders()), response.read()
        finally:
            connection.close()

    def queue(self, path, body, **options):
        self.responses.append((path, body, options))

    def assert_genesis(self, genesis=GENESIS):
        status, headers, body = self.request()
        self.assertEqual(status, 200, body[:500])
        self.assertEqual(body, genesis)
        self.assertEqual(headers['Content-Type'], 'application/json')
        self.assertEqual(headers['X-LayerX-Genesis-SHA256'], hashlib.sha256(genesis).hexdigest())
        self.assertEqual(int(headers['Content-Length']), len(genesis))
        self.assertEqual(headers['Cache-Control'], 'no-store')
        self.assertEqual(json.loads(body)['chain_id'], 'pacific-1')
        self.assertEqual(self.responses, [])

    def assert_invalid(self):
        status, headers, body = self.request()
        self.assertEqual(status, 502, body[:500])
        self.assertNotIn('X-LayerX-Genesis-SHA256', headers)
        self.assertEqual(json.loads(body)['error']['code'], 'comet_response_invalid')

    def test_direct_verbatim_and_http_chunked(self):
        for wrapped in (False, True):
            for framing in (False, True):
                with self.subTest(wrapped=wrapped, http_chunked=framing):
                    self.queue('/genesis', direct(wrapped=wrapped), http_chunked=framing)
                    self.assert_genesis()
        self.assertEqual(self.calls, ['/genesis'] * 4)

    def test_ordered_chunked_verbatim(self):
        for wrapped in (False, True):
            genesis = b' \n' + GENESIS + b'\n '
            parts = [genesis[:17], genesis[17:101], genesis[101:]]
            self.queue('/genesis', rpc(CHUNKED_ERROR, error=True) if wrapped else encoded(CHUNKED_ERROR))
            for index, part in enumerate(parts):
                self.queue(f'/genesis_chunked?chunk={index}', chunk(part, index, len(parts), wrapped))
            self.assert_genesis(genesis)
        self.assertEqual(self.calls, ['/genesis', '/genesis_chunked?chunk=0',
                                     '/genesis_chunked?chunk=1', '/genesis_chunked?chunk=2'] * 2)

    def test_no_configuration_and_route_restrictions(self):
        for path, method in [('/genesis', 'POST'), ('/genesis/', 'GET'), ('/genesis_chunked', 'GET')]:
            self.assertEqual(self.request(path, method)[0], 404)
        self.assertEqual(self.request('/genesis?chunk=0')[0], 400)
        for method in ['eth_sendTransaction', 'eth_sign', 'admin_nodeInfo', 'genesis', 'genesis_chunked']:
            status, _, body = self.request('/', 'POST', encoded({'jsonrpc': '2.0', 'id': 1,
                                                               'method': method, 'params': []}))
            self.assertEqual(status, 200)
            self.assertEqual(json.loads(body)['error']['code'], -32601)
        self.start_boundary(None)
        status, headers, body = self.request()
        self.assertEqual(status, 404)
        self.assertEqual(json.loads(body), {'error': {'code': 'not_found', 'retry': 'never'}})
        self.assertNotIn('X-LayerX-Genesis-SHA256', headers)
        self.assertEqual(self.calls, [])

    def test_errors_do_not_trigger_chunked(self):
        for body in [b'bad json', b'[]', b'{}', direct(b'null'), direct(b'{"chain_id":""}'),
                     rpc({'code': -32603, 'message': 'Internal error', 'data': 'unrelated'}, error=True),
                     encoded(CHUNKED_ERROR | {'code': -32600}), rpc(CHUNKED_ERROR),
                     rpc({'genesis': json.loads(GENESIS)}, error=True),
                     rpc({'genesis': json.loads(GENESIS)})[:-1],
                     b'{"jsonrpc":"1.0","result":{}}', b'{"result":{}}']:
            with self.subTest(body=body[:80]):
                self.queue('/genesis', body)
                self.assert_invalid()
        self.queue('/genesis', encoded(CHUNKED_ERROR), status=500)
        self.assert_invalid()
        self.assertTrue(all(path == '/genesis' for path in self.calls))

    def test_bad_chunk_metadata_and_encoding(self):
        good = json.loads(chunk(GENESIS))
        for update in [{'total': '0'}, {'total': '33'}, {'total': '-1'}, {'total': 1},
                       {'chunk': '1'}, {'chunk': '-1'}, {'chunk': 0}, {'data': ''},
                       {'data': '!!!!'}, {'data': 'A==='}, {'data': 'AB=='}, {'data': 'AAB='},
                       {'data': 'AA==AAAA'}, {'data': 'AAA'}, {'data': 'AA=A'}]:
            with self.subTest(update=update):
                self.queue('/genesis', encoded(CHUNKED_ERROR))
                self.queue('/genesis_chunked?chunk=0', encoded(good | update))
                self.assert_invalid()
        for second in [chunk(GENESIS[10:], 1, 3), chunk(GENESIS[10:], 0, 2), b'{}',
                       rpc(json.loads(chunk(GENESIS[10:], 1, 2)), error=True),
                       chunk(b'not-json', 1, 2)]:
            self.queue('/genesis', encoded(CHUNKED_ERROR))
            self.queue('/genesis_chunked?chunk=0', chunk(GENESIS[:10], 0, 2))
            self.queue('/genesis_chunked?chunk=1', second)
            self.assert_invalid()

    def test_bounds_and_truncation(self):
        self.queue('/genesis', b'', length=LIMIT + 64 * 1024 + 1)
        self.assert_invalid()
        self.queue('/genesis', direct(), length=len(direct()) + 1)
        self.assert_invalid()
        for size in [LIMIT, LIMIT + 1]:
            genesis = GENESIS[:-1] + b' ' * (size - len(GENESIS)) + b'}'
            self.queue('/genesis', direct(genesis))
            if size == LIMIT:
                self.assert_genesis(genesis)
            else:
                self.assert_invalid()
        genesis = GENESIS + b' ' * (LIMIT - len(GENESIS))
        for extra in [b'', b' ']:
            self.queue('/genesis', encoded(CHUNKED_ERROR))
            self.queue('/genesis_chunked?chunk=0', chunk(genesis[:LIMIT//2], 0, 2))
            self.queue('/genesis_chunked?chunk=1', chunk(genesis[LIMIT//2:] + extra, 1, 2))
            if extra:
                self.assert_invalid()
            else:
                self.assert_genesis(genesis)

    def test_deadline_and_unreachable(self):
        self.queue('/genesis', direct(), delay=31)
        started = time.monotonic()
        self.assert_invalid()
        self.assertLess(time.monotonic() - started, 32)
        with socket.socket() as reserved:
            reserved.bind(('127.0.0.1', 0))
            self.start_boundary(f'http://127.0.0.1:{reserved.getsockname()[1]}')
            status, headers, body = self.request()
        self.assertEqual(status, 503)
        self.assertNotIn('X-LayerX-Genesis-SHA256', headers)
        self.assertEqual(json.loads(body)['error']['code'], 'comet_unavailable')

    def test_invalid_endpoint_rejected_at_startup(self):
        for url in ['', 'https://127.0.0.1:26657', 'http://192.0.2.1:26657',
                    'http://localhost:26657', 'http://127.0.0.1:0', 'http://127.0.0.1:26657/path',
                    'http://user@127.0.0.1:26657', 'http://127.0.0.1:26657?x',
                    'http://127.0.0.1:26657/\r\n']:
            result = subprocess.run([sys.argv[1]], env=self.env | {
                'LAYERX_PAXEER_NODE_URL': 'http://127.0.0.1:1', 'LAYERX_PAXEER_COMET_URL': url},
                capture_output=True, timeout=5)
            self.assertEqual(result.returncode, 2, url)
            self.assertIn(b'LAYERX_PAXEER_COMET_URL', result.stderr)


if __name__ == '__main__':
    unittest.main(argv=[sys.argv[0]], verbosity=2)
