import concurrent.futures
import json
import socket
import ssl
import sys

context = ssl.create_default_context(cafile=sys.argv[2])
port = int(sys.argv[1])


def probe(identifier):
    try:
        with socket.create_connection(('127.0.0.1', port), timeout=10) as tcp:
            with context.wrap_socket(tcp, server_hostname='localhost') as tls:
                body = json.dumps({
                    'jsonrpc': '2.0', 'id': identifier, 'method': 'eth_getBalance',
                    'params': ['0x0000000000000000000000000000000000000001', 'latest'],
                }).encode()
                tls.sendall(
                    b'POST / HTTP/1.1\r\nHost: localhost\r\n'
                    b'Content-Type: application/json\r\nContent-Length: '
                    + str(len(body)).encode() + b'\r\n\r\n' + body)
                data = b''
                while b'\r\n\r\n' not in data:
                    part = tls.recv(4096)
                    assert part, 'EOF before response headers'
                    data += part
                headers, data = data.split(b'\r\n\r\n', 1)
                assert headers.split(b'\r\n', 1)[0] == b'HTTP/1.1 200 OK'
                size = int(next(
                    line.split(b':', 1)[1] for line in headers.split(b'\r\n')
                    if line.lower().startswith(b'content-length:')))
                while len(data) < size:
                    part = tls.recv(4096)
                    assert part, 'EOF before response body'
                    data += part
                assert len(data) == size
                response = json.loads(data)
                assert response['id'] == identifier and 'result' in response
                tls.unwrap().close()
        return None
    except Exception as error:
        return type(error).__name__ + ': ' + str(error)


with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
    results = list(pool.map(probe, range(1000)))
errors = [error for error in results if error]
print(json.dumps({
    'requests': len(results), 'passed': len(results) - len(errors),
    'errors': errors[:20],
}, indent=2))
sys.exit(bool(errors))
