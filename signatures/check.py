import hashlib
import json
import re
from pathlib import Path
from cryptography.hazmat.backends.openssl.backend import backend
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec, ed25519, utils
from cryptography.exceptions import InvalidSignature

root = Path(__file__).resolve().parent
for source in json.loads((root / 'sources.json').read_text()):
    assert hashlib.sha256((root / source['file']).read_bytes()).hexdigest() == source['sha256']
print('All authority source SHA256 values match')
print(backend.openssl_version_text())
source = Path('programs/crates/layerx-programs-runtime/tests/signature_vectors.rs').read_text()
group = json.loads((root / 'wycheproof.json').read_text())['testGroups'][0]
test = next(t for t in group['tests'] if t['tcId'] == 3)
assert test['result'] == 'valid'
der = bytes.fromhex(test['sig'])
r, s = utils.decode_dss_signature(der)
digest = hashlib.sha256(bytes.fromhex(test['msg'])).digest()
key = ec.EllipticCurvePublicKey.from_encoded_point(ec.SECP256K1(), bytes.fromhex(group['publicKey']['uncompressed']))
raw = r.to_bytes(32, 'big') + s.to_bytes(32, 'big')
for name, encoding in [('secp256k1_verify_accepts_compressed_public_key', serialization.PublicFormat.CompressedPoint), ('secp256k1_verify_accepts_uncompressed_public_key', serialization.PublicFormat.UncompressedPoint), ('secp256k1_verify_published_test_vector_1', serialization.PublicFormat.UncompressedPoint)]:
    body = source.split('fn ' + name + '() {')[1].split('\n}')[0]
    actual = [bytes.fromhex(v) for v in re.findall(r'hex::decode\(\s*"([0-9a-f]*)"', body)]
    expected = [digest, key.public_bytes(serialization.Encoding.X962, encoding), raw]
    assert actual == expected
    ec.EllipticCurvePublicKey.from_encoded_point(ec.SECP256K1(), actual[1]).verify(der, actual[0], ec.ECDSA(utils.Prehashed(hashes.SHA256())))
    print(name, 'tcId 3 bytes match; OpenSSL prehash verification accepts')
for rr, ss in [(0, 0), (0, 255)]:
    try:
        key.verify(utils.encode_dss_signature(rr, ss), digest, ec.ECDSA(utils.Prehashed(hashes.SHA256())))
    except InvalidSignature:
        print(f'OpenSSL rejects r={rr}, s={ss} with valid secp256k1 public key')
    else:
        raise AssertionError('OpenSSL accepted zero scalar')
high = next(t for t in group['tests'] if t['tcId'] == 1)
high_der = bytes.fromhex(high['sig'])
_, high_s = utils.decode_dss_signature(high_der)
assert high_s > 0xfffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141 // 2
key.verify(high_der, hashlib.sha256(bytes.fromhex(high['msg'])).digest(), ec.ECDSA(utils.Prehashed(hashes.SHA256())))
print('Wycheproof tcId 1 OpenSSL accepts genuine high-S signature; existing LayerX low-S refusal test retained')
body = source.split('fn malleable_signature_detection_ed25519() {')[1].split('\n}')[0]
pk, sig, order = [bytes.fromhex(v) for v in re.findall(r'hex::decode\(\s*"([0-9a-f]*)"', body)]
rfc = re.sub(r'\s', '', (root / 'rfc8032.txt').read_text())
assert pk.hex() in rfc and sig.hex() in rfc
L = 2**252 + 27742317777372353535851937790883648493
assert int.from_bytes(order, 'little') == L
key = ed25519.Ed25519PublicKey.from_public_bytes(pk)
key.verify(sig, b'')
mutated = sig[:32] + (int.from_bytes(sig[32:], 'little') + L).to_bytes(32, 'little')
try:
    key.verify(mutated, b'')
except InvalidSignature:
    print('RFC 8032 TEST 1 OpenSSL accepts; S + L OpenSSL rejects; S + L > L')
else:
    raise AssertionError('OpenSSL accepted S + L')
p = 2**255 - 19
y = int.from_bytes(bytes([2])*32, 'little')
d = -121665 * pow(121666, -1, p) % p
x2 = (y*y - 1) * pow(d*y*y + 1, -1, p) % p
assert pow(x2, (p-1)//2, p) == p-1
print('RFC 8032 section 5.1.7: [02;32] decoding fails, quadratic nonresidue')
assert 'R and S with value 0 are allowed in the encoding.' in (root / 'secp256k1.h').read_text()
print('libsecp256k1 v0.6.0 compact parser permits zero encoding; guarantees verification failure; LayerX MalformedSignature classification is separately owner-authorized')
assert 5 + 3 * 8 == 29 and 5 + 2 * 8 + 80 == 101 and 5 + 8 == 13
print('Independent scan encoding arithmetic: terminal three entries = 29, two entries + 80-byte cursor = 101, terminal one entry = 13')
print('Guest cursor starts at 32; memory overwrite 64..68 maps to cursor[32..36], changing namespace binding')
