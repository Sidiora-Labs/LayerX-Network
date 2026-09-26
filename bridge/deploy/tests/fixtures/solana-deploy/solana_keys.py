"""Solana key arithmetic for the offline deployment check: base58, the public
key of a keypair file, and Pubkey::find_program_address, with no dependency
beyond the Python standard library.

  solana_keys.py pubkey <keypair.json>
  solana_keys.py hex <base58 key>
  solana_keys.py base58 <hex>
  solana_keys.py pda <program id> <seed>...   seeds are string:<text> or pubkey:<base58>
"""

import hashlib
import json
import sys

ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
FIELD = 2**255 - 19
EDWARDS_D = (-121665 * pow(121666, FIELD - 2, FIELD)) % FIELD
PDA_MARKER = b"ProgramDerivedAddress"


def encode(raw):
    number = int.from_bytes(raw, "big")
    text = ""
    while number:
        number, remainder = divmod(number, 58)
        text = ALPHABET[remainder] + text
    return "1" * (len(raw) - len(raw.lstrip(b"\x00"))) + text


def decode(text):
    number = 0
    for character in text:
        position = ALPHABET.find(character)
        if position < 0:
            raise SystemExit("%s carries %r, which is not a base58 character" % (text, character))
        number = number * 58 + position
    raw = number.to_bytes((number.bit_length() + 7) // 8, "big")
    raw = b"\x00" * (len(text) - len(text.lstrip("1"))) + raw
    if len(raw) != 32:
        raise SystemExit("%s decodes to %d bytes, not 32" % (text, len(raw)))
    return raw


def on_curve(raw):
    # The compressed Edwards y decompresses exactly when (y^2 - 1) / (d y^2 + 1)
    # has a square root in the field.
    y = (int.from_bytes(raw, "little") & ((1 << 255) - 1)) % FIELD
    u = (y * y - 1) % FIELD
    v = (EDWARDS_D * y * y + 1) % FIELD
    square = u * pow(v, FIELD - 2, FIELD) % FIELD
    return square == 0 or pow(square, (FIELD - 1) // 2, FIELD) == 1


def seed_bytes(seed):
    kind, _, value = seed.partition(":")
    if kind == "string":
        return value.encode("utf-8")
    if kind == "pubkey":
        return decode(value)
    raise SystemExit("%s is neither string:<text> nor pubkey:<base58>" % seed)


def find_program_address(seeds, program_id):
    for bump in range(255, -1, -1):
        candidate = hashlib.sha256(b"".join(seeds) + bytes([bump]) + program_id + PDA_MARKER).digest()
        if not on_curve(candidate):
            return candidate, bump
    raise SystemExit("no bump seed derives an address off the curve")


def main(argv):
    if len(argv) >= 2 and argv[0] == "pubkey":
        with open(argv[1], encoding="utf-8") as handle:
            raw = bytes(json.load(handle))
        if len(raw) != 64:
            raise SystemExit("%s holds %d bytes, not a 64-byte keypair" % (argv[1], len(raw)))
        print(encode(raw[32:]))
    elif len(argv) == 2 and argv[0] == "hex":
        print(decode(argv[1]).hex())
    elif len(argv) == 2 and argv[0] == "base58":
        print(encode(bytes.fromhex(argv[1])))
    elif len(argv) >= 3 and argv[0] == "pda":
        address, bump = find_program_address([seed_bytes(seed) for seed in argv[2:]], decode(argv[1]))
        print("%s %d" % (encode(address), bump))
    else:
        raise SystemExit(__doc__)


if __name__ == "__main__":
    main(sys.argv[1:])
