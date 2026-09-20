#!/usr/bin/env python3
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import struct
import subprocess

ROOT = Path(__file__).resolve().parents[1]
LIMIT = 50 * 1024 * 1024
PART_LIMIT = 24_000_000


def members(raw):
    if raw[:8] != b"!<arch>\n":
        raise ValueError("archive magic")
    offset, names, table = 8, b"", b""
    entries = []
    while offset < len(raw):
        header = raw[offset:offset + 60]
        if len(header) != 60 or header[58:] != b"`\n":
            raise ValueError("archive member header")
        size = int(header[48:58])
        end = offset + 60 + size
        if end + size % 2 > len(raw):
            raise ValueError("archive member length")
        block, body = raw[offset:end + size % 2], raw[offset + 60:end]
        name = header[:16].decode("ascii").strip()
        offset = end + size % 2
        if name == "//":
            names, table = body, block
            continue
        if name in ("/", "/SYM64/"):
            continue
        if name.startswith("#1/"):
            width = int(name[3:])
            name, body = body[:width].rstrip(b"\0").decode(), body[width:]
        elif name.startswith("/"):
            name = names[int(name[1:]):].split(b"/\n", 1)[0].decode()
        else:
            name = name.removesuffix("/")
        if name.startswith("__.SYMDEF"):
            continue
        entries.append((name, body, block))
    return table, entries


def identity(raw):
    return [(name, len(body), hashlib.sha256(body).hexdigest())
            for name, body, _ in members(raw)[1]]


def write_part(directory, name, raw, architecture):
    if len(raw) > LIMIT:
        raise ValueError("packaged archive exceeds publication bound")
    (directory / name).write_bytes(raw)
    return {"name": name, "size": len(raw), "sha256": hashlib.sha256(raw).hexdigest(),
            "architecture": architecture}


def split_archive(directory, name, raw):
    table, entries = members(raw)
    groups, current, size = [], [], 0
    for entry in entries:
        if current and size + len(entry[2]) > PART_LIMIT:
            groups.append(current)
            current, size = [], 0
        current.append(entry)
        size += len(entry[2])
    if current:
        groups.append(current)
    parts, reconstructed = [], []
    for index, group in enumerate(groups):
        part_name = name.removesuffix(".a") + f".part{index:02}.a"
        path = directory / part_name
        path.write_bytes(b"!<arch>\n" + table + b"".join(entry[2] for entry in group))
        subprocess.run([os.environ.get("LLVM_AR", "llvm-ar"), "s", str(path)], check=True)
        encoded = path.read_bytes()
        reconstructed.extend(identity(encoded))
        parts.append(write_part(directory, part_name, encoded, 0))
    if reconstructed != identity(raw):
        raise ValueError("archive member identity or ordering changed")
    return parts


def package(directory, name):
    path = directory / name
    compressed_path = directory / (name + ".gz")
    candidate = path.read_bytes() if path.exists() else b""
    if candidate.startswith((b"!<arch>\n", b"\xca\xfe\xba\xbe")):
        raw = candidate
    else:
        raw = gzip.decompress(compressed_path.read_bytes())
    compressed = io.BytesIO()
    with gzip.GzipFile(filename="", mode="wb", fileobj=compressed, compresslevel=6, mtime=0) as stream:
        stream.write(raw)
    encoded = compressed.getvalue()
    if len(encoded) > LIMIT or gzip.decompress(encoded) != raw:
        raise ValueError("compressed archive bound or identity")
    compressed_path.write_bytes(encoded)
    entry = {"name": name, "size": len(raw), "sha256": hashlib.sha256(raw).hexdigest(),
             "compressed_size": len(encoded), "compressed_sha256": hashlib.sha256(encoded).hexdigest()}
    if raw[:4] == b"\xca\xfe\xba\xbe":
        count = struct.unpack_from(">I", raw, 4)[0]
        parts = []
        for index in range(count):
            cpu, _, offset, size, _ = struct.unpack_from(">IIIII", raw, 8 + index * 20)
            suffix = {0x1000007: "amd64", 0x100000C: "arm64"}[cpu]
            part = raw[offset:offset + size]
            if len(part) != size:
                raise ValueError("fat archive slice bounds")
            members(part)
            parts.append(write_part(directory, name.removesuffix(".a") + f".{suffix}.a", part, cpu))
        entry.update(kind="fat", parts=parts)
        path.unlink(missing_ok=True)
    else:
        parts = split_archive(directory, name, raw)
        linux = "muslc" in name
        entry.update(kind="group" if linux else "darwin", parts=parts)
        if linux:
            path.write_text("GROUP ( " + " ".join("-l:" + part["name"] for part in parts) + " )\n")
        else:
            path.unlink(missing_ok=True)
    return entry


for location, prefix in [
    ("wasm-runtime/internal/api", "libwasmvm"),
    ("wasm/x/wasm/artifacts/v152/api", "libwasmvm152"),
    ("wasm/x/wasm/artifacts/v155/api", "libwasmvm155"),
]:
    directory = ROOT / location
    entries = [package(directory, name) for name in [
        prefix + "_muslc.a", prefix + "_muslc.aarch64.a", prefix + "static_darwin.a"]]
    (directory / "static-archives.json").write_text(json.dumps({"version": 1, "archives": entries}, indent=2) + "\n")
    (directory / "link_muslc.go").write_text(
        "//go:build linux && muslc && !sys_wasmvm\n\npackage api\n\n"
        "// #cgo LDFLAGS: -Wl,-rpath,${SRCDIR} -L${SRCDIR}\n"
        f"// #cgo amd64 LDFLAGS: -l{prefix.removeprefix('lib')}_muslc\n"
        f"// #cgo arm64 LDFLAGS: -l:{prefix}_muslc.aarch64.a\n"
        'import "C"\n')
    flags = []
    for part in entries[2]["parts"]:
        architecture = {0: "", 0x1000007: "amd64 ", 0x100000C: "arm64 "}[part["architecture"]]
        flags.append(f"// #cgo {architecture}LDFLAGS: -l{part['name'].removeprefix('lib').removesuffix('.a')}\n")
    (directory / "link_mac_static.go").write_text(
        "//go:build darwin && static_wasm && !sys_wasmvm\n\npackage api\n\n"
        "// #cgo LDFLAGS: -L${SRCDIR}\n" + "".join(flags) + 'import "C"\n')
