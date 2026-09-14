import os
from pathlib import Path
import stat
import struct
import subprocess
import sys


def crc32c(data):
    crc = 0xffffffff
    for byte in data:
        crc ^= byte
        for _ in range(8):
            crc = (crc >> 1) ^ (0x82f63b78 if crc & 1 else 0)
    return (~crc) & 0xffffffff


assert len(sys.argv) == 3
path = Path(sys.argv[1])
builder = Path(sys.argv[2]).resolve(strict=True)
descriptor = os.open(path, os.O_RDWR | os.O_NOFOLLOW | os.O_CLOEXEC)
with os.fdopen(descriptor, 'r+b') as log:
    info = os.fstat(log.fileno())
    assert stat.S_ISREG(info.st_mode) and info.st_nlink == 1
    header = log.read(32)
    assert len(header) == 32 and header[:8] == b'LXPL\x05\0\0\0'
    assert int.from_bytes(header[8:16], 'big') == 1
    length, crc = struct.unpack_from('>II', header, 16)
    assert 0 < length <= 1024 * 1024 and length + 32 <= info.st_size
    body = log.read(length)
    assert len(body) == length and crc32c(body) == crc and body[:5] == b'LXBE1'
    prefix = header + body
    retained = path.with_suffix('.retained-prefix')
    with retained.open('xb') as evidence:
        evidence.write(prefix)
        evidence.flush()
        os.fsync(evidence.fileno())
    with retained.open('rb') as evidence:
        assert evidence.read() == prefix
    replacement = path.with_suffix('.rebuilt-prefix')
    subprocess.run([str(builder), str(retained), str(replacement), str(info.st_size)], check=True)
    with replacement.open('rb') as rebuilt:
        assert os.fstat(rebuilt.fileno()).st_size == info.st_size
        assert rebuilt.read(len(prefix)) == prefix
    os.chown(replacement, info.st_uid, info.st_gid)
    os.chmod(replacement, stat.S_IMODE(info.st_mode))
    os.replace(replacement, path)
    directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
    try:
        os.fsync(directory)
    finally:
        os.close(directory)
print('Stopped replica retains exactly its first authenticated receipt record')
