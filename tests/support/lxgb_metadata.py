import hashlib


def metadata(asset, issuer_public, salt):
    if len(asset) != 32 or len(issuer_public) != 32 or len(salt) != 32:
        raise ValueError('asset, issuer public key and supplied salt must be 32 bytes')
    did = ('did:layerx:' + issuer_public.hex()).encode()
    issuer = hashlib.sha256(b'LXP/v1/did-id\0' + len(did).to_bytes(2, 'big') + did).digest()
    record = (b'\0\x03' + asset + b'\x03TST\x06\x01\0\0\0\x0dCustody token'
              + bytes(16) + issuer + b'\x02' + bytes(16) + salt)
    schedule = b'\0\x02' + bytes(80) + (10000).to_bytes(4, 'big') + b'\x08'
    schedule += b''.join((0).to_bytes(16, 'big') for _ in range(8))
    return b'\0\x01' + len(record).to_bytes(2, 'big') + record + len(schedule).to_bytes(2, 'big') + schedule
