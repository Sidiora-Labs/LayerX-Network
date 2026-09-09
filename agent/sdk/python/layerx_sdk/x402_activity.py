import hashlib


def bind_receive_activity(
    canonical: bytes, receive: bytes, actor: str, network: int, key: str
) -> str:
    offset = 0

    def take(size):
        nonlocal offset
        if size < 0 or offset + size > len(canonical):
            raise ValueError("invalid-activity")
        value = canonical[offset : offset + size]
        offset += size
        return value

    def integer(size):
        return int.from_bytes(take(size), "big")

    def field(tag):
        if integer(1) != tag:
            raise ValueError("invalid-activity-field")

    def bounded(maximum):
        size = integer(4)
        if size > maximum:
            raise ValueError("invalid-activity-length")
        return take(size)

    if (
        type(canonical) is not bytes
        or len(canonical) > 524288
        or type(network) is not int
    ):
        raise ValueError("invalid-activity")
    version = integer(2)
    if version not in (1, 2, 3, 4) or integer(2) != 0x1001 or integer(1) != 12:
        raise ValueError("invalid-activity-header")
    field(1)
    if integer(2) != version:
        raise ValueError("invalid-activity-version")
    field(2)
    if integer(4) != network:
        raise ValueError("activity-network-mismatch")
    field(3)
    if integer(4) != 0x10006:
        raise ValueError("activity-type-mismatch")
    field(4)
    if bounded(255).decode("utf-8") != actor:
        raise ValueError("activity-actor-mismatch")
    field(5)
    if not bounded(524288):
        raise ValueError("missing-activity-authority")
    field(6)
    integer(8)
    field(7)
    before = integer(8)
    if integer(8) < before:
        raise ValueError("invalid-activity-time")
    field(8)
    if bounded(32).hex() != key:
        raise ValueError("activity-key-mismatch")
    field(9)
    integer(16)
    field(10)
    payload_hash = bounded(32)
    field(11)
    payload = bounded(524288)
    field(12)
    if len(bounded(128)) != 64 or offset != len(canonical):
        raise ValueError("invalid-activity-signature")
    if (
        payload != receive
        or payload_hash != hashlib.sha256(b"LXP/v1/payload-hash\0" + payload).digest()
    ):
        raise ValueError("activity-payload-mismatch")
    return hashlib.sha256(b"LXP/v1/activity-id\0" + canonical).hexdigest()
