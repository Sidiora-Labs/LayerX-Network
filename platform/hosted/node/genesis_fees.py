import argparse
from pathlib import Path
import sys


def withdrawal_metadata(metadata, price):
    if type(price) is not int or not 0 <= price < 2**64:
        raise ValueError('withdrawal price must be a uint64')
    if not 2 <= len(metadata) <= 16384:
        raise ValueError('genesis metadata is outside request bounds')
    count = int.from_bytes(metadata[:2], 'big')
    if count == 0:
        raise ValueError('genesis metadata requires asset records')
    cursor = 2
    for _ in range(count):
        if cursor + 2 > len(metadata):
            raise ValueError('truncated asset record length')
        length = int.from_bytes(metadata[cursor:cursor + 2], 'big')
        cursor += 2
        if length == 0 or length > len(metadata) - cursor:
            raise ValueError('truncated asset record')
        cursor += length
    if cursor + 2 > len(metadata):
        raise ValueError('missing native fee schedule')
    length = int.from_bytes(metadata[cursor:cursor + 2], 'big')
    schedule = metadata[cursor + 2:]
    if length != len(schedule):
        raise ValueError('noncanonical native fee schedule length')
    if length == 255 and schedule[:2] == b'\0\x03' and schedule[86] == 11:
        if int.from_bytes(schedule[247:], 'big') != price:
            raise ValueError('withdrawal price disagrees with supplied v3 metadata')
        return metadata
    if length != 247 or schedule[:2] != b'\0\x02' or schedule[86] != 10:
        raise ValueError('withdrawal configuration requires a canonical v2 or v3 schedule')
    expanded = b'\0\x03' + schedule[2:86] + b'\x0b' + schedule[87:] + price.to_bytes(8, 'big')
    return metadata[:cursor] + len(expanded).to_bytes(2, 'big') + expanded


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('metadata', type=Path)
    parser.add_argument('price', type=int)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    encoded = withdrawal_metadata(args.metadata.read_bytes(), args.price)
    if not args.check:
        sys.stdout.buffer.write(encoded)


if __name__ == '__main__':
    main()
