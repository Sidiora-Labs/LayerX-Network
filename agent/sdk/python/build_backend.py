from __future__ import annotations

import base64
import csv
import hashlib
import io
import zipfile
from pathlib import Path

NAME = "layerx_sdk"
VERSION = "0.1.0"
DIST_INFO = f"{NAME}-{VERSION}.dist-info"
WHEEL_NAME = f"{NAME}-{VERSION}-py3-none-any.whl"
ZIP_TIME = (1980, 1, 1, 0, 0, 0)


def _metadata() -> bytes:
    return (
        "Metadata-Version: 2.3\n"
        "Name: layerx-sdk\n"
        f"Version: {VERSION}\n"
        "Summary: Generated LayerX Agent API Python SDK\n"
        "Requires-Python: >=3.11\n"
        "License: See the LayerX repository LICENSE\n"
        "\n"
    ).encode()


def _wheel() -> bytes:
    return (
        b"Wheel-Version: 1.0\n"
        b"Generator: layerx-sdk deterministic backend 1\n"
        b"Root-Is-Purelib: true\n"
        b"Tag: py3-none-any\n"
        b"\n"
    )


def _source_files(root: Path) -> dict[str, bytes]:
    files: dict[str, bytes] = {}
    for path in sorted((root / "layerx_sdk").rglob("*")):
        if path.is_file() and "__pycache__" not in path.parts and path.suffix != ".pyc":
            files[path.relative_to(root).as_posix()] = path.read_bytes()
    data_root = f"{NAME}-{VERSION}.data/data/share/layerx-sdk/examples"
    for path in sorted((root / "examples").glob("*.py")):
        files[f"{data_root}/{path.name}"] = path.read_bytes()
    files[f"{DIST_INFO}/METADATA"] = _metadata()
    files[f"{DIST_INFO}/WHEEL"] = _wheel()
    return files


def _record(files: dict[str, bytes]) -> bytes:
    output = io.StringIO(newline="")
    writer = csv.writer(output, lineterminator="\n")
    for name, content in sorted(files.items()):
        digest = base64.urlsafe_b64encode(hashlib.sha256(content).digest()).rstrip(b"=").decode()
        writer.writerow((name, f"sha256={digest}", len(content)))
    writer.writerow((f"{DIST_INFO}/RECORD", "", ""))
    return output.getvalue().encode()


def _write_entry(archive: zipfile.ZipFile, name: str, content: bytes) -> None:
    info = zipfile.ZipInfo(name, ZIP_TIME)
    info.compress_type = zipfile.ZIP_DEFLATED
    info.create_system = 3
    info.external_attr = 0o100644 << 16
    archive.writestr(info, content, compresslevel=9)


def build_wheel(
    wheel_directory: str,
    config_settings: object | None = None,
    metadata_directory: str | None = None,
) -> str:
    del config_settings, metadata_directory
    root = Path(__file__).resolve().parent
    destination = Path(wheel_directory)
    destination.mkdir(parents=True, exist_ok=True)
    files = _source_files(root)
    files[f"{DIST_INFO}/RECORD"] = _record(files)
    wheel_path = destination / WHEEL_NAME
    with zipfile.ZipFile(wheel_path, "w") as archive:
        for name, content in sorted(files.items()):
            _write_entry(archive, name, content)
    return WHEEL_NAME


def get_requires_for_build_wheel(config_settings: object | None = None) -> list[str]:
    del config_settings
    return []
