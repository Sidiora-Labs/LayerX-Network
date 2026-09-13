#!/usr/bin/env python3
import argparse
import hashlib
import json
import os
from pathlib import Path
import secrets
import shlex
import signal
import stat
import struct
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", required=True)
    args = parser.parse_args()
    os.setsid()
    config = json.loads(Path(args.config).read_text())
    root = Path(config["root"]).resolve(strict=True)
    runtime = root / "registry-runtime"
    runtime.mkdir(mode=0o750)
    os.chown(runtime, 0, 4030)
    state = runtime / "state"
    state.mkdir(mode=0o700)
    os.chown(state, 4030, 4030)
    image = subprocess.check_output(
        ["docker", "image", "inspect", "--format", "{{.Id}}",
         "layerx-repair/registry-runtime:20260913"], text=True).strip()
    isolation_digest = subprocess.check_output(
        ["docker", "run", "--rm", image, "sha256sum", "/usr/bin/bwrap"],
        text=True).split()[0]
    bins = Path(config["service_bin_dir"]).resolve(strict=True)
    supervisor = bins / "layerx-cgroup-exec"
    supervisor_digest = hashlib.sha256(supervisor.read_bytes()).hexdigest()
    mounts = []
    mounted = {}

    def mount_file(source):
        source = Path(source)
        if source.resolve(strict=True) != source or not source.is_file():
            raise RuntimeError("registry input must be a canonical regular file")
        metadata = source.stat()
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
            raise RuntimeError("registry input must be singly linked")
        if source not in mounted:
            os.chown(source, metadata.st_uid, 4030)
            os.chmod(source, 0o640)
            target = "/run/layerx/inputs/" + str(len(mounted))
            mounts.extend(["--mount", f"type=bind,src={source},dst={target},readonly"])
            mounted[source] = target
        return mounted[source]

    def generated(name, data):
        target = runtime / name
        descriptor = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, "wb") as output:
            output.write(data)
            output.flush()
            os.fsync(output.fileno())
        return mount_file(target)

    history = b"LayerX/sequencer-trust-history/v1\0"
    history += struct.pack(">HHHIQ", 1, 0, 3, config["network_id"], config["epoch"])
    history += bytes.fromhex(config["sequencer_id"])
    history += bytes.fromhex(config["sequencer_public_key"])
    history += struct.pack(">QQ", 1, (1 << 64) - 1) + bytes(9)
    certificates = Path(config["certificates_dir"])
    ca = mount_file(certificates / "ca.der")
    p12 = mount_file(config["client_pkcs12"])
    password = mount_file(config["client_password_file"])
    environment = {
        "LAYERX_REGISTRY_LISTEN": f"0.0.0.0:{config['listen_port']}",
        "LAYERX_REGISTRY_HOST_CGROUP_MOUNT": "/run/layerx/host-cgroup",
        "LAYERX_REGISTRY_STATE": "/run/layerx/state",
        "LAYERX_REGISTRY_BUILD_ROOT": "/run/layerx/quota",
        "LAYERX_REGISTRY_MAX_BUILDS": "1",
        "LAYERX_REGISTRY_BUILDER_IMAGE_DIGEST": Path(config["builder_digest_file"]).read_text().strip(),
        "LAYERX_REGISTRY_BUILDER_ENVIRONMENT_ROOT": "/run/layerx/builder",
        "LAYERX_REGISTRY_BUILDER_ENTRYPOINT": "/bin/layerx-build",
        "LAYERX_REGISTRY_BUILDER_ISOLATION_RUNTIME": "/usr/bin/bwrap",
        "LAYERX_REGISTRY_BUILDER_ISOLATION_RUNTIME_DIGEST": isolation_digest,
        "LAYERX_REGISTRY_BUILDER_JOB_SUPERVISOR": "/run/layerx/bin/layerx-cgroup-exec",
        "LAYERX_REGISTRY_BUILDER_JOB_SUPERVISOR_DIGEST": supervisor_digest,
        "LAYERX_REGISTRY_REQUEST_TOKEN_FILE": mount_file(config["request_token_file"]),
        "LAYERX_REGISTRY_PUBLICATION_TOKEN_FILE": generated("publication-token", secrets.token_hex(32).encode()),
        "LAYERX_REGISTRY_TLS_CERT_DER": mount_file(certificates / "core.der"),
        "LAYERX_REGISTRY_TLS_KEY_DER": mount_file(certificates / "core-key.der"),
        "LAYERX_REGISTRY_CLIENT_CA_DER": ca,
        "LAYERX_REGISTRY_OUTBOUND_CA_DER": ca,
        "LAYERX_REGISTRY_NODE_ENDPOINT": config["node_url"],
        "LAYERX_REGISTRY_RECEIPT_AUTHORITY_ENDPOINT": config["authority_url"],
        "LAYERX_REGISTRY_RECEIPT_AUTHORITY_REPLICA_ID": config["replica_id"],
        "LAYERX_REGISTRY_SEQUENCER_TRUST_HISTORY": generated("trust-history", history),
        "LAYERX_REGISTRY_IDENTITY_URL": config["identity_url"],
        "LAYERX_REGISTRY_IDENTITY_TOKEN_FILE": mount_file(config["identity_token_file"]),
        "LAYERX_REGISTRY_IDENTITY_CA_DER": ca,
        "LAYERX_REGISTRY_IDENTITY_CLIENT_IDENTITY_PKCS12": p12,
        "LAYERX_REGISTRY_IDENTITY_CLIENT_IDENTITY_PASSWORD_FILE": password,
    }
    for name, value in config["event_environment"].items():
        if name.startswith(("LAYERX_EVENTS_PROGRAM_", "LAYERX_EVENTS_WEBHOOKS_")):
            environment[name] = mount_file(value) if value.startswith("/") else value
    node_token = mount_file(config["node_token_file"])
    authority_token = mount_file(config["authority_token_file"])
    script = "#!/bin/sh\nset -eu\n"
    script += "\n".join(f"export {name}={shlex.quote(value)}" for name, value in environment.items()) + "\n"
    script += f'export LAYERX_REGISTRY_NODE_AUTHORIZATION="Bearer $(cat {shlex.quote(node_token)})"\n'
    script += f'export LAYERX_REGISTRY_RECEIPT_AUTHORITY_AUTHORIZATION="Bearer $(cat {shlex.quote(authority_token)})"\n'
    script += "exec /run/layerx/bin/layerx-program-registry\n"
    entrypoint = generated("entrypoint.sh", script.encode())
    name = f"layerx-registry-test-{os.getpid()}-{secrets.token_hex(4)}"
    command = ["docker", "run", "--rm", "--name", name, "--network", "host",
               "--cgroupns", "private", "--cap-drop", "ALL",
               "--cap-add", "CHOWN", "--cap-add", "SETUID", "--cap-add", "SETGID",
               "--security-opt", "no-new-privileges"]
    for source, target, readonly in [
        ("/sys/fs/cgroup", "/run/layerx/host-cgroup", False),
        (state, "/run/layerx/state", False),
        ("/root/lx-builds/repair-registry-runtime/quota", "/run/layerx/quota", False),
        (config["builder_root"], "/run/layerx/builder", True),
        (bins / "layerx-program-registry", "/run/layerx/bin/layerx-program-registry", True),
        (supervisor, "/run/layerx/bin/layerx-cgroup-exec", True),
    ]:
        command.extend(["--mount", f"type=bind,src={source},dst={target}" + (",readonly" if readonly else "")])
    command.extend(mounts)
    command.extend([image, "/bin/sh", entrypoint])
    process = None

    def stop(_signal, _frame):
        raise SystemExit(0)

    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    try:
        process = subprocess.Popen(command)
        return process.wait()
    finally:
        subprocess.run(["docker", "stop", "--time", "5", name],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
        if process is not None:
            process.wait(timeout=10)


if __name__ == "__main__":
    sys.exit(main())
