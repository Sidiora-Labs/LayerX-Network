import os
from pathlib import Path
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time


def exact(connection, size):
    value = bytearray()
    while len(value) != size:
        part = connection.recv(size - len(value))
        if not part:
            raise EOFError
        value.extend(part)
    return bytes(value)


def frame(connection):
    prefix = exact(connection, 4)
    size = struct.unpack('>I', prefix)[0]
    assert 22 <= size <= 2 * 1024 * 1024
    return prefix + exact(connection, size)


class Proxy:
    def __init__(self, path, upstream, mode):
        self.upstream, self.mode = upstream, mode
        self.stop = threading.Event()
        self.requests, self.responses, self.failures = [], [], []
        self.listener = socket.socket(socket.AF_UNIX)
        self.listener.bind(str(path))
        self.listener.listen(4)
        self.listener.settimeout(.1)
        self.thread = threading.Thread(target=self.serve)
        self.thread.start()

    def serve(self):
        try:
            while not self.stop.is_set():
                try:
                    client, _ = self.listener.accept()
                except TimeoutError:
                    continue
                with client, socket.socket(socket.AF_UNIX) as daemon:
                    client.settimeout(2)
                    daemon.settimeout(2)
                    daemon.connect(self.upstream)
                    try:
                        while not self.stop.is_set():
                            request = frame(client)
                            tag = struct.unpack('>H', request[8:10])[0]
                            if tag == 28:
                                self.requests.append(request[18:])
                            daemon.sendall(request)
                            response = frame(daemon)
                            reply_tag = struct.unpack('>H', response[8:10])[0]
                            self.responses.append(reply_tag)
                            if ((self.mode == 'ack' and reply_tag == 29 and
                                 len(self.requests) == 1) or
                                (self.mode == 'read' and reply_tag == 15 and
                                 self.responses.count(15) == 1)):
                                break
                            if self.mode == 'deadline' and reply_tag == 29:
                                self.stop.wait(.2)
                                break
                            client.sendall(response)
                    except (EOFError, BrokenPipeError, ConnectionResetError):
                        pass
        except BaseException as error:
            self.failures.append(error)

    def close(self):
        self.stop.set()
        self.thread.join(3)
        assert not self.thread.is_alive(), 'transparent LNI proxy did not stop'
        self.listener.close()
        assert not self.failures, self.failures


def exercise(directory, executable, upstream, batch, previous=None):
    certificate, proof = directory / 'checkpoint', directory / 'finality'
    conflict = directory / 'conflicting-finality'
    altered = bytearray(proof.read_bytes())
    altered[-1] ^= 1
    conflict.write_bytes(altered)
    if previous is not None:
        proxy = Proxy(directory / 'previous.sock', upstream, 'ack')
        try:
            result = subprocess.run([str(previous), str(directory / 'previous.sock'),
                str(batch), str(certificate), str(proof), '30000', 'accepted'],
                capture_output=True, text=True, timeout=32)
        finally:
            proxy.close()
        assert result.returncode == -6, result
        assert 'feedback result=-904 expected=0' in result.stdout, result
        assert len(proxy.requests) == 1 and proxy.responses[-1] == 29
        print('previous production feedback sequence refuses the lost real acknowledgement', flush=True)
    for mode in ('ack', 'read', 'conflict', 'deadline', 'direct'):
        path = directory / (mode + '.sock')
        proxy = Proxy(path, upstream, mode)
        timeout = 700 if mode == 'deadline' else 30000
        expected = mode if mode in ('conflict', 'deadline') else 'accepted'
        started = time.monotonic()
        try:
            subprocess.run([str(executable), str(path), str(batch), str(certificate),
                str(conflict if mode == 'conflict' else proof), str(timeout), expected],
                check=True, timeout=timeout / 1000 + 2)
            elapsed = time.monotonic() - started
        finally:
            proxy.close()
        if mode in ('ack', 'read'):
            assert len(proxy.requests) == 2, proxy.requests
            assert proxy.responses.count(29) == 2
        if mode == 'read':
            assert proxy.responses.count(15) == 2
        if mode == 'conflict':
            assert len(proxy.requests) == 1 and proxy.responses[-1] == 25
        if mode == 'deadline':
            assert len(proxy.requests) >= 2
            assert timeout / 1000 <= elapsed < timeout / 1000 + .5, elapsed
        assert proxy.requests and all(value == proxy.requests[0] for value in proxy.requests)
        print(f'actual LNI feedback {mode}: requests={len(proxy.requests)} elapsed={elapsed:.3f}s', flush=True)


def run(executable, upstream, batch, certificate, proof, output):
    with tempfile.TemporaryDirectory(prefix='lxp-feedback-', dir='/tmp') as temporary:
        directory = Path(temporary)
        os.chown(directory, 4021, 4021)
        directory.chmod(0o700)
        for source, name in ((executable, 'probe'), (Path(__file__), 'transport.py'),
                             (certificate, 'checkpoint'), (proof, 'finality')):
            destination = directory / name
            shutil.copyfile(source, destination)
            os.chown(destination, 4021, 4021)
            destination.chmod(0o700 if name == 'probe' else 0o400)
        previous = os.environ.get('LAYERX_TEST_GUARANTOR_FEEDBACK_PREVIOUS_BIN')
        if previous is not None:
            destination = directory / 'previous'
            shutil.copyfile(previous, destination)
            os.chown(destination, 4021, 4021)
            destination.chmod(0o700)
        with Path(output).open('wb') as log:
            subprocess.run(['setpriv', '--reuid=4021', '--regid=4021', '--clear-groups',
                '/usr/bin/python3', str(directory / 'transport.py'), str(directory),
                str(directory / 'probe'), upstream, str(batch),
                *([str(directory / 'previous')] if previous is not None else [])], stdout=log, stderr=log,
                check=True, timeout=180)
    print(Path(output).read_text(), end='', flush=True)


if __name__ == '__main__':
    assert len(sys.argv) in (5, 6)
    exercise(Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3], int(sys.argv[4]),
             Path(sys.argv[5]) if len(sys.argv) == 6 else None)
