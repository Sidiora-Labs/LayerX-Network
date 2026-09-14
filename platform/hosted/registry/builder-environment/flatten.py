#!/usr/bin/env python3
"""Turn an exported container filesystem into the regular-files-only tree the
registry's environment digest accepts: every symlink is replaced by a copy of
what it resolves to inside the tree, dangling links and special files are
removed, caches and documentation are dropped, and every write and set-id bit
is cleared."""
import os, shutil, stat, sys

root = os.path.abspath(sys.argv[1])
PRUNE = [
    "usr/share/doc", "usr/share/man", "usr/share/info", "usr/share/locale",
    "usr/share/lintian", "usr/share/bug", "var/cache", "var/lib/apt", "var/log",
    "var/tmp", "tmp", "run", "proc", "sys", "dev", "root/.cache", "root/.npm",
    "usr/share/zoneinfo", "usr/lib/x86_64-linux-gnu/gconv",
    "usr/share/pixmaps", "usr/share/applications", "usr/share/icons",
    "opt/node/lib/node_modules/npm/docs", "opt/node/lib/node_modules/npm/man",
    "opt/cache/rustup/toolchains/1.91.1-x86_64-unknown-linux-gnu/share/doc",
    "opt/cache/rustup/toolchains/1.91.1-x86_64-unknown-linux-gnu/share/man",
]
for rel in PRUNE:
    path = os.path.join(root, rel)
    if os.path.islink(path):
        os.unlink(path)
    elif os.path.isdir(path):
        shutil.rmtree(path)
    elif os.path.exists(path):
        os.unlink(path)


def resolve(link):
    """Resolve one symlink chain inside the tree; None when it leaves the tree
    or dangles."""
    seen = 0
    current = link
    while os.path.islink(current):
        target = os.readlink(current)
        if target.startswith("/"):
            current = os.path.join(root, target.lstrip("/"))
        else:
            current = os.path.normpath(os.path.join(os.path.dirname(current), target))
        if not (current == root or current.startswith(root + os.sep)):
            return None
        seen += 1
        if seen > 40:
            return None
    return current if os.path.exists(current) else None


# Debian's merged-usr makes /bin, /sbin, /lib and /lib64 symlinks into /usr.
# Copying them whole doubles the tree, so only the paths binaries and scripts
# hard-code are materialised: the ELF interpreter and the POSIX shell. The
# dynamic loader then finds libraries through its default /usr search path
# once the stale merged-usr cache is gone.
MERGED = {
    "bin": ["sh", "dash", "layerx-build"],
    "sbin": [],
    "lib": [],
    "lib32": [],
    "libx32": [],
    "lib64": ["ld-linux-x86-64.so.2"],
}
planned = []
for name, keep in MERGED.items():
    top = os.path.join(root, name)
    if not os.path.islink(top):
        continue
    target = os.path.join(root, os.readlink(top).lstrip("/"))
    copies = []
    for entry in keep:
        source = os.path.join(target, entry)
        resolved = resolve(source) if os.path.islink(source) else source
        if resolved is not None and os.path.isfile(resolved):
            with open(resolved, "rb") as handle:
                copies.append((entry, handle.read(), os.lstat(resolved).st_mode))
    planned.append((top, copies))
for top, copies in planned:
    os.unlink(top)
    if not copies:
        continue
    os.mkdir(top)
    for entry, data, mode in copies:
        path = os.path.join(top, entry)
        with open(path, "wb") as handle:
            handle.write(data)
        os.chmod(path, mode & 0o777)
for rel in ("etc/ld.so.cache",):
    path = os.path.join(root, rel)
    if os.path.exists(path):
        os.unlink(path)


for _round in range(64):
    links = []
    for current, dirs, files in os.walk(root):
        for name in dirs + files:
            full = os.path.join(current, name)
            if os.path.islink(full):
                links.append(full)
    if not links:
        break
    links.sort(key=lambda p: p.count(os.sep))
    for link in links:
        if not os.path.islink(link):
            continue
        target = resolve(link)
        os.unlink(link)
        if target is None:
            continue
        if os.path.isdir(target):
            shutil.copytree(target, link, symlinks=True)
        else:
            shutil.copy2(target, link, follow_symlinks=False)
else:
    sys.exit("symlink replacement did not converge")

# Mount points the registry sandbox binds over the read-only root.
for rel in ("build", "tmp", "proc", "dev"):
    os.makedirs(os.path.join(root, rel), exist_ok=True)

for current, dirs, files in os.walk(root, topdown=False):
    for name in files:
        full = os.path.join(current, name)
        st = os.lstat(full)
        if not stat.S_ISREG(st.st_mode):
            os.unlink(full)
            continue
        mode = st.st_mode & 0o777
        mode &= ~(0o222)
        os.chmod(full, mode)
    for name in dirs:
        full = os.path.join(current, name)
        if os.path.islink(full):
            os.unlink(full)
            continue
        os.chmod(full, (os.lstat(full).st_mode & 0o777) & ~0o222)
os.chmod(root, (os.lstat(root).st_mode & 0o777) & ~0o222)

entries = 0
total = 0
for current, dirs, files in os.walk(root):
    entries += len(dirs) + len(files)
    for name in files:
        st = os.lstat(os.path.join(current, name))
        if not stat.S_ISREG(st.st_mode) or st.st_mode & 0o6222:
            sys.exit("non-regular or writable entry survived: %s" % os.path.join(current, name))
        total += st.st_size
print("entries=%d bytes=%d" % (entries, total))
if entries > 60000 or total > 4 * 1024 ** 3:
    sys.exit("tree exceeds the builder slot bounds")
