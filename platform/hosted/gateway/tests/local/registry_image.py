#!/usr/bin/env python3
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import secrets
import signal
import subprocess
import tarfile


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--image', required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()

    def stop(number, _frame):
        raise SystemExit(128 + number)

    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    image = subprocess.check_output(
        ['docker', 'image', 'inspect', '--format', '{{.Id}}', args.image], text=True).strip()
    container = f'layerx-registry-image-{os.getpid()}-{secrets.token_hex(4)}'
    try:
        subprocess.run(['docker', 'create', '--name', container, image],
                       check=True, stdout=subprocess.DEVNULL)
        archive = subprocess.check_output(['docker', 'cp', f'{container}:/usr/bin/bwrap', '-'])
        with tarfile.open(fileobj=io.BytesIO(archive), mode='r:*') as files:
            members = files.getmembers()
            if len(members) != 1 or not members[0].isfile():
                raise RuntimeError('image isolation executable must be one regular file')
            executable = files.extractfile(members[0])
            if executable is None:
                raise RuntimeError('image isolation executable is missing')
            digest = hashlib.file_digest(executable, 'sha256').hexdigest()
    finally:
        subprocess.run(['docker', 'rm', container], check=True, stdout=subprocess.DEVNULL)
    descriptor = os.open(args.output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, 'w') as output:
        json.dump({'image_id': image, 'isolation_sha256': digest}, output, sort_keys=True)
        output.write('\n')
        output.flush()
        os.fsync(output.fileno())
    directory = os.open(args.output.parent, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(directory)
    finally:
        os.close(directory)


if __name__ == '__main__':
    main()
