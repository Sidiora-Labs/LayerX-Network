#!/usr/bin/env python3
import hashlib, os, struct, sys
root = sys.argv[1]
entries = []
for current, dirs, files in os.walk(root):
    dirs.sort()
    for name in dirs + files:
        full = os.path.join(current, name)
        rel = os.path.relpath(full, root)
        st = os.lstat(full)
        if os.path.islink(full) or not (os.path.isdir(full) or os.path.isfile(full)):
            sys.exit("builder environment contains a non-regular entry: %s" % rel)
        entries.append((tuple(rel.split(os.sep)), rel, os.path.isdir(full), 0 if os.path.isdir(full) else st.st_mode))
if len(entries) > 100_000:
    sys.exit("builder environment exceeds 100000 entries")
entries.sort()
digest = hashlib.sha256(b"LayerX/hosted-builder/environment/v1\0")
total = 0
for _, rel, is_dir, mode in entries:
    name = rel.encode()
    digest.update(struct.pack(">Q", len(name)) + name + bytes([1 if is_dir else 0]) + struct.pack(">I", mode & 0xFFFFFFFF))
    if is_dir:
        digest.update(struct.pack(">Q", 0))
        continue
    with open(os.path.join(root, rel), "rb") as handle:
        data = handle.read()
    total += len(data)
    if total > 4 << 30:
        sys.exit("builder environment exceeds 4 GiB")
    digest.update(struct.pack(">Q", len(data)) + data)
print(digest.hexdigest())
