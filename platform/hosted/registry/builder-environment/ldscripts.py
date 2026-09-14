#!/usr/bin/env python3
"""glibc's linker scripts (libc.so, libm.so) name their members by the
absolute merged-usr path /lib/x86_64-linux-gnu/...; materialise exactly those
files under /lib so a host link inside the sandbox resolves them."""
import os, re, shutil, stat, sys
root = os.path.abspath(sys.argv[1])
root_mode = stat.S_IMODE(os.lstat(root).st_mode)
os.chmod(root, root_mode | stat.S_IWUSR)
libdir = os.path.join(root, "usr/lib/x86_64-linux-gnu")
wanted = set()
for name in os.listdir(libdir):
    path = os.path.join(libdir, name)
    if not os.path.isfile(path) or os.path.getsize(path) > 4096:
        continue
    with open(path, "rb") as handle:
        head = handle.read(4096)
    if b"GNU ld script" not in head and b"GROUP" not in head:
        continue
    wanted.update(re.findall(rb"/lib/x86_64-linux-gnu/[A-Za-z0-9_.+-]+", head))
for rel in sorted(wanted):
    rel = rel.decode()
    source = os.path.join(root, "usr" + rel)
    target = os.path.join(root, rel.lstrip("/"))
    if not os.path.isfile(source):
        sys.exit("linker script member missing from usr: %s" % rel)
    os.makedirs(os.path.dirname(target), exist_ok=True)
    shutil.copy2(source, target, follow_symlinks=False)
    os.chmod(target, os.lstat(target).st_mode & 0o555)
    print(rel)
for current, dirs, _files in os.walk(os.path.join(root, "lib")):
    os.chmod(current, os.lstat(current).st_mode & 0o555)
os.chmod(root, root_mode)
