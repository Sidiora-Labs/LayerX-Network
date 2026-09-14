import os
from pathlib import Path
import stat
import struct
import sys


def crc32c(data):
    crc = 0xffffffff
    for byte in data:
        crc ^= byte
        for _ in range(8):
            crc = (crc >> 1) ^ (0x82f63b78 if crc & 1 else 0)
    return (~crc) & 0xffffffff


path = Path(sys.argv[1])
assert len(sys.argv) == 2
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
    with path.with_suffix('.retained-prefix').open('xb') as evidence:
        evidence.write(prefix)
        evidence.flush()
        os.fsync(evidence.fileno())
    log.truncate(len(prefix))
    log.flush()
    os.fsync(log.fileno())
    log.seek(0)
    assert log.read() == prefix
print('Stopped replica retains exactly its first authenticated receipt record')
