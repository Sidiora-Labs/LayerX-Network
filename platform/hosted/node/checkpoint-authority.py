#!/usr/bin/env python3
import fcntl
import os
from pathlib import Path
import stat
import subprocess
import sys


def public_key(path):
    path = Path(path)
    lock = os.open(str(path) + '.lock', os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    with os.fdopen(lock, 'rb') as guard:
        fcntl.flock(guard, fcntl.LOCK_EX)
        if not path.exists() and not path.is_symlink():
            descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
            with os.fdopen(descriptor, 'wb') as destination:
                subprocess.run(['openssl', 'genpkey', '-algorithm', 'ED25519'],
                               stdout=destination, stderr=subprocess.PIPE, check=True)
                destination.flush()
                os.fsync(destination.fileno())
            directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
            try:
                os.fsync(directory)
            finally:
                os.close(directory)
        metadata = path.lstat()
        if not stat.S_ISREG(metadata.st_mode) or stat.S_IMODE(metadata.st_mode) != 0o600 or metadata.st_uid != os.geteuid():
            raise ValueError('checkpoint authority key must be an owned regular 0600 file')
        public = subprocess.run(['openssl', 'pkey', '-in', str(path), '-pubout', '-outform', 'DER'],
                                capture_output=True, check=True).stdout
        if len(public) != 44 or public[:12] != bytes.fromhex('302a300506032b6570032100'):
            raise ValueError('checkpoint authority must be Ed25519')
        return '0x' + public[12:].hex()


if __name__ == '__main__':
    try:
        if len(sys.argv) != 2:
            raise ValueError('usage: checkpoint-authority.py KEY_FILE')
        print(public_key(sys.argv[1]))
    except Exception:
        sys.exit('checkpoint authority provisioning refused')
