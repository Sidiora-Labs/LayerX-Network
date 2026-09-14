import argparse
import json
from pathlib import Path
import sys

MODULE_NAMES = ('escrow', 'budget', 'stream', 'service', 'perps', 'governance', 'bridge')


def module_prices(path):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError('duplicate module fee')
            result[key] = value
        return result
    values = json.loads(Path(path).read_text(), object_pairs_hook=unique)
    if type(values) is not dict or set(values) != set(MODULE_NAMES):
        raise ValueError('module fee configuration must name exactly modules 2 through 8')
    prices = tuple(values[name] for name in MODULE_NAMES)
    if any(type(price) is not int or not 0 <= price < 2**128 for price in prices):
        raise ValueError('module fee prices must be uint128')
    return prices


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
    if ((length == 255 and schedule[:2] == b'\0\x03' and schedule[86] == 11)
            or (length == 368 and schedule[:2] == b'\0\x04' and schedule[86] == 11 and schedule[255] == 7)):
        if int.from_bytes(schedule[247:255], 'big') != price:
            raise ValueError('withdrawal price disagrees with supplied metadata')
        return metadata
    if length != 247 or schedule[:2] != b'\0\x02' or schedule[86] != 10:
        raise ValueError('withdrawal configuration requires a canonical v2 or v3 schedule')
    expanded = b'\0\x03' + schedule[2:86] + b'\x0b' + schedule[87:] + price.to_bytes(8, 'big')
    return metadata[:cursor] + len(expanded).to_bytes(2, 'big') + expanded


def module_metadata(metadata, withdrawal_price, prices):
    if len(prices) != 7 or any(type(price) is not int or not 0 <= price < 2**128 for price in prices):
        raise ValueError('exactly seven uint128 module prices are required')
    canonical = withdrawal_metadata(metadata, withdrawal_price)
    cursor = 2
    for _ in range(int.from_bytes(canonical[:2], 'big')):
        cursor += 2 + int.from_bytes(canonical[cursor:cursor + 2], 'big')
    schedule = canonical[cursor + 2:]
    tail = b'\x07' + b''.join(price.to_bytes(16, 'big') for price in prices)
    if schedule[:2] == b'\0\x04':
        if schedule[255:] != tail:
            raise ValueError('module prices disagree with supplied v4 metadata')
        return canonical
    expanded = b'\0\x04' + schedule[2:] + tail
    result = canonical[:cursor] + len(expanded).to_bytes(2, 'big') + expanded
    if len(result) > 16384:
        raise ValueError('module fee metadata exceeds request bounds')
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('metadata', type=Path)
    parser.add_argument('price', type=int)
    parser.add_argument('--check', action='store_true')
    parser.add_argument('--module-fees', type=Path)
    args = parser.parse_args()
    encoded = withdrawal_metadata(args.metadata.read_bytes(), args.price)
    if args.module_fees is not None:
        encoded = module_metadata(encoded, args.price, module_prices(args.module_fees))
    if not args.check:
        sys.stdout.buffer.write(encoded)


if __name__ == '__main__':
    main()
