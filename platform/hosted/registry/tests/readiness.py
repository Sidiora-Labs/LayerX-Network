#!/usr/bin/env python3
"""Real-process readiness qualification; requires the real node and build boundary."""
import argparse
import concurrent.futures
import json
import os
from pathlib import Path
import shutil
import ssl
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import urllib.parse


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True)
    parser.add_argument('--rootfs', required=True)
    parser.add_argument('--url', required=True)
    parser.add_argument('--ca', required=True)
    parser.add_argument('--cert', required=True)
    parser.add_argument('--key', required=True)
    parser.add_argument('--token-file', required=True)
    parser.add_argument('--build-path', required=True)
    parser.add_argument('--build-body', required=True)
    parser.add_argument('--log', required=True)
    args = parser.parse_args()
    origin = urllib.parse.urlsplit(args.url)
    try:
        with socket.create_connection((origin.hostname, origin.port or 443), timeout=1):
            raise RuntimeError('test URL already has a listener')
    except ConnectionRefusedError:
        pass
    context = ssl.create_default_context(cafile=args.ca)
    context.load_cert_chain(args.cert, args.key)
    token = Path(args.token_file).read_text().strip()
    body = Path(args.build_body).read_bytes()

    def request(path, data=None):
        req = urllib.request.Request(args.url + path, data=data,
            headers={'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json'})
        started = time.monotonic()
        try:
            with urllib.request.urlopen(req, context=context, timeout=1800 if data is not None else 8) as response:
                return response.status, time.monotonic() - started
        except urllib.error.HTTPError as error:
            return error.code, time.monotonic() - started

    with tempfile.TemporaryDirectory(prefix='registry-readiness-') as temporary:
        os.chmod(temporary, 0o755)
        root = Path(temporary) / 'rootfs'
        shutil.copytree(args.rootfs, root)
        if os.geteuid() == 0:
            for path in [root, *root.rglob('*')]:
                os.chown(path, 4030, 4030)
        env = os.environ.copy()
        env['LAYERX_REGISTRY_BUILDER_ENVIRONMENT_ROOT'] = str(root)
        with open(args.log, 'wb') as log:
            process = subprocess.Popen([args.binary], env=env, stdout=log, stderr=log)
            try:
                limit = time.monotonic() + 180
                while True:
                    if process.poll() is not None:
                        raise RuntimeError('registry exited during startup')
                    try:
                        if request('/healthz')[0] == 200:
                            break
                    except (OSError, urllib.error.URLError):
                        pass
                    if time.monotonic() >= limit:
                        raise RuntimeError('registry startup deadline exceeded')
                    time.sleep(0.1)
                with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
                    builds = [pool.submit(request, args.build_path, body) for _ in range(2)]
                    latencies = []
                    for _ in range(20):
                        assert any(not build.done() for build in builds), 'build concurrency not established'
                        status, elapsed = request('/healthz')
                        assert status == 200, status
                        assert elapsed < 1, elapsed
                        latencies.append(elapsed)
                    assert any(build.result()[0] == 200 for build in builds), 'no real build succeeded'
                changed = next(path for path in root.rglob('*') if path.is_file() and path.stat().st_size)
                with changed.open('r+b') as output:
                    first = output.read(1)
                    output.seek(0)
                    output.write(bytes([first[0] ^ 1]))
                    output.flush()
                    os.fsync(output.fileno())
                limit = time.monotonic() + 3
                while request('/healthz')[0] != 503:
                    assert time.monotonic() < limit, 'mutated rootfs remained healthy'
                    time.sleep(0.05)
                assert request(args.build_path, body)[0] == 503
                print(json.dumps({'health_samples': len(latencies), 'max_seconds': max(latencies),
                    'mutated_health': 503, 'mutated_build': 503}))
            finally:
                if process.poll() is None:
                    process.terminate()
                process.wait(timeout=10)


if __name__ == '__main__':
    main()
