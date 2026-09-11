import hashlib

VECTOR_ASSET = bytes(range(0, 32))
VECTOR_ISSUER_PUBLIC = bytes(range(32, 64))
VECTOR_SALT = bytes(range(64, 96))
VECTOR_LENGTH = 439
VECTOR_SHA256 = '38d5d09e3fc241bd6b6f924e4fe656f48954a61055ddec048f1eb636a72c8fe4'


def custody_reference(asset):
    reference = bytes(12) + asset[12:]
    if not any(reference):
        raise ValueError('paxeer custody reference must be non-zero')
    return reference


def metadata(asset, issuer_public, salt):
    if len(asset) != 32 or len(issuer_public) != 32 or len(salt) != 32:
        raise ValueError('asset, issuer public key and supplied salt must be 32 bytes')
    did = ('did:layerx:' + issuer_public.hex()).encode()
    issuer = hashlib.sha256(b'LXP/v1/did-id\0' + len(did).to_bytes(2, 'big') + did).digest()
    reference = custody_reference(asset)
    record = (b'\0\x03' + asset + b'\x03TST\x06\x02' + len(reference).to_bytes(2, 'big') + reference
              + b'\0\x0dCustody token' + bytes(16) + issuer + b'\x02' + bytes(16) + salt)
    schedule = b'\0\x02' + bytes(80) + (10000).to_bytes(4, 'big') + b'\x0a'
    schedule += b''.join((0).to_bytes(16, 'big') for _ in range(10))
    return b'\0\x01' + len(record).to_bytes(2, 'big') + record + len(schedule).to_bytes(2, 'big') + schedule


def check():
    encoded = metadata(VECTOR_ASSET, VECTOR_ISSUER_PUBLIC, VECTOR_SALT)
    digest = hashlib.sha256(encoded).hexdigest()
    if len(encoded) != VECTOR_LENGTH or digest != VECTOR_SHA256:
        raise AssertionError('lxgb metadata emitter drift: length=%d sha256=%s; '
                             'tests/support/lxgb_metadata.rs pins the same vector'
                             % (len(encoded), digest))
    return encoded


if __name__ == '__main__':
    check()
    print('lxgb metadata vector %s length %d' % (VECTOR_SHA256, VECTOR_LENGTH))
