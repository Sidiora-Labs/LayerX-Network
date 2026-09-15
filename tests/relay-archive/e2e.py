#!/usr/bin/env python3
import hashlib
import json
import os
from pathlib import Path
import select
import shutil
import socket
import ssl
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
BUILD = Path(sys.argv[1] if len(sys.argv) > 1 else ROOT / "build").resolve()
sys.path.insert(0, str(ROOT / "tests/support"))
from lxgb_metadata import metadata
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def unused_port():
    with socket.socket() as connection:
        connection.bind(("127.0.0.1", 0))
        return connection.getsockname()[1]


def execute(args, *, data=None, env=None, uid=None, success=True):
    identity = {} if uid is None else {"user": uid, "group": uid, "extra_groups": []}
    result = subprocess.run([str(arg) for arg in args], input=data, env=env,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            timeout=90, cwd=ROOT, **identity)
    if success and result.returncode:
        raise AssertionError(f"{Path(args[0]).name} failed ({result.returncode}): "
                             + result.stderr.decode(errors="replace")[-6000:])
    if not success:
        require(result.returncode != 0, "invalid input unexpectedly accepted")
    return result


def write_json(path, value):
    path.write_text(json.dumps(value, sort_keys=True) + "\n")
    path.chmod(0o644)


def env_file(path):
    return dict(line.split("=", 1) for line in path.read_text().splitlines() if line)


class Scenario:
    def __init__(self):
        self.work = Path(tempfile.mkdtemp(prefix="layerx-relay-archive-e2e-"))
        self.work.chmod(0o755)
        self.children = []
        self.logs = []
        self.context = None
        self.checks = []

    def passed(self, message):
        self.checks.append(message)
        print("PASS " + message, flush=True)

    def start(self, name, args, *, env=None, uid=None, pass_fds=()):
        output = (self.work / (name + ".log")).open("wb")
        identity = {} if uid is None else {"user": uid, "group": uid, "extra_groups": []}
        process = subprocess.Popen([str(arg) for arg in args], stdout=output,
                                   stderr=subprocess.STDOUT, env=env, cwd=ROOT,
                                   pass_fds=pass_fds, **identity)
        self.children.append(process)
        self.logs.append(output)
        return process

    def stop(self, process):
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=10)

    def close(self):
        for process in reversed(self.children):
            self.stop(process)
        for output in self.logs:
            output.close()
        print("Evidence: " + str(self.work), flush=True)

    def request(self, base, path, *, data=None, headers=None, status=200, raw=False):
        request = urllib.request.Request(base + path, data=data, headers=headers or {})
        try:
            response = urllib.request.urlopen(request, context=self.context, timeout=8)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            body = response.read()
            require(response.status == status,
                    f"{path}: expected HTTP {status}, got {response.status}: {body[:500]!r}")
        return body if raw else json.loads(body)

    def until(self, condition, label, seconds=30):
        deadline = time.monotonic() + seconds
        last = None
        while time.monotonic() < deadline:
            try:
                value = condition()
                if value:
                    return value
                last = f"condition returned {value!r}"
            except (OSError, AssertionError, urllib.error.URLError) as error:
                last = error
            time.sleep(0.1)
        raise AssertionError(f"timed out: {label}; last error: {last}")

    def setup(self):
        require(os.geteuid() == 0, "real daemon LNI UID-isolation scenario requires root")
        self.bin = self.work / "bin"
        self.bin.mkdir()
        for name in ("layerxd", "layerx-genesis-build", "layerx-archive-codec"):
            shutil.copy2(BUILD / "bin" / name, self.bin / name)
        shutil.copy2(BUILD / "tests/relay-archive-sign", self.bin / "sign")
        self.runtime = self.work / "runtime"
        shutil.copytree(ROOT / "platform/relay_archive", self.runtime,
                        ignore=shutil.ignore_patterns("__pycache__"))
        for name, seed in (("sequencer", 0x22), ("treasury", 0x11)):
            path = self.work / name
            path.write_bytes(bytes([seed]) * 32)
            path.chmod(0o600)
        issuer = Ed25519PrivateKey.from_private_bytes(bytes([0x11]) * 32).public_key()
        issuer = issuer.public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
        asset = bytes.fromhex("b5a32b12029f8ddfb905f90f280f664b46390de0fc62770fc197dd87b18cd898")
        (self.work / "metadata").write_bytes(metadata(asset, issuer, os.urandom(32)))
        self.data = self.work / "native"
        self.run = self.work / "run"
        program_port, replica_port = unused_port(), unused_port()
        environment = dict(os.environ,
            LAYERX_NODE_PAXEER_CHAIN_ID="31337",
            LAYERX_NODE_SETTLEMENT_CONTRACT="0x" + "11" * 20,
            LAYERX_NODE_CHECKPOINT_REGISTRY="0x" + "22" * 20,
            LAYERX_NODE_PAXEER_RPC_ADDRESS="127.0.0.1",
            LAYERX_NODE_PAXEER_RPC_PORT=str(unused_port()))
        execute(["bash", ROOT / "platform/hosted/node/bootstrap.sh",
                 "--data-dir", self.data, "--run-dir", self.run, "--network-id", "77",
                 "--genesis-metadata", self.work / "metadata",
                 "--sequencer-key", self.work / "sequencer", "--treasury-key", self.work / "treasury",
                 "--lni-uid", "4021", "--lni-gid", "4021", "--program-port", str(program_port),
                 "--replica-port", str(replica_port), "--layerxd", self.bin / "layerxd",
                 "--genesis-build", self.bin / "layerx-genesis-build"], env=environment)
        self.node = env_file(self.data / "node.env")
        ready_read, ready_write = os.pipe()
        replica_env = dict(os.environ, **env_file(self.data / "replica.env"),
                           LAYERX_AUTHORITY_READY_FD=str(ready_write))
        self.start("native-replica", [self.bin / "layerxd", "--authority-replica",
                                     self.data / "replica.conf"], env=replica_env, pass_fds=(ready_write,))
        os.close(ready_write)
        require(bool(select.select([ready_read], [], [], 25)[0]) and os.read(ready_read, 1) == b"R",
                "native authority replica failed readiness")
        os.close(ready_read)
        sequencer_env = dict(os.environ, **env_file(self.data / "sequencer.env"),
                             LAYERX_NODE_SEQUENCER_PRIVATE_KEY=(bytes([0x22]) * 32).hex())
        self.start("native-sequencer", [self.bin / "layerxd", "--serve", self.data / "sequencer.conf"],
                   env=sequencer_env)
        self.socket = self.run / "layerxd.lni.sock"
        self.until(lambda: self.socket.exists(), "native LNI socket")
        self.activity0 = execute([self.bin / "sign", "0"]).stdout
        ack = json.loads(execute([self.bin / "layerx-archive-codec", "submit", self.socket],
                                 data=self.activity0, uid=4021).stdout)
        require(ack["state"] == "acknowledged", "native submission did not acknowledge")
        self.id0 = ack["activity_id"]
        self.source_log = self.data / "checkpoints/da-bodies.log"
        self.until(lambda: self.source_log.exists() and self.source_log.stat().st_size > 0,
                   "real native canonical availability body")
        for path in (self.data, self.data / "checkpoints", self.data / "genesis"):
            path.chmod(0o755)
        self.source_log.chmod(0o644)
        self.manifest = self.data / "genesis/genesis.manifest"
        self.snapshot = self.data / "genesis/00000000000000000000.lxs"
        self.manifest.chmod(0o644)
        self.snapshot.chmod(0o644)
        self.token = self.work / "submission-token"
        self.token.write_text("relay-e2e-" + os.urandom(32).hex())
        os.chown(self.token, 4021, 4021)
        self.token.chmod(0o600)
        execute(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes",
                 "-keyout", self.work / "tls.key", "-out", self.work / "tls.crt",
                 "-days", "1", "-subj", "/CN=localhost",
                 "-addext", "subjectAltName=IP:127.0.0.1,DNS:localhost"])
        os.chown(self.work / "tls.key", 4021, 4021)
        self.context = ssl.create_default_context(cafile=str(self.work / "tls.crt"))
        self.pins = {
            "network_id": 77,
            "genesis_sha256": hashlib.sha256(self.manifest.read_bytes()).hexdigest(),
            "sequencer_id": self.node["LAYERX_NODE_SEQUENCER_ID"],
            "sequencer_public_key": self.node["LAYERX_NODE_SEQUENCER_PUBLIC_KEY"],
            "sequencer_first_batch": 1,
            "allow_loopback_dev": True,
            "poll_interval_seconds": 0.2,
            "codec": str(self.bin / "layerx-archive-codec"),
            "ca_file": str(self.work / "tls.crt"),
        }
        self.origin1, self.origin1_process = self.origin("origin-one")
        self.origin2, self.origin2_process = self.origin("origin-two")
        self.headers = {"Authorization": "Bearer " + self.token.read_text(),
                        "Content-Type": "application/octet-stream"}
        self.passed("signed native activity sequenced and public HTTPS canonical origin synchronized")

    def origin(self, name):
        port = unused_port()
        url = f"https://127.0.0.1:{port}"
        directory = self.work / name
        directory.mkdir()
        os.chown(directory, 4021, 4021)
        config = dict(self.pins, data_dir=str(directory), listen=f"127.0.0.1:{port}", public_url=url,
                      genesis_manifest=str(self.manifest), genesis_snapshot=str(self.snapshot),
                      source_log=str(self.source_log), source_lni_socket=str(self.socket),
                      source_submission_token_file=str(self.token),
                      tls_cert=str(self.work / "tls.crt"), tls_key=str(self.work / "tls.key"))
        path = self.work / (name + ".json")
        write_json(path, config)
        process = self.start(name, [self.bin / "layerxd", "--relay-archive", path], uid=4021,
                             env=dict(os.environ, LAYERX_RELAY_ARCHIVE_RUNTIME=str(self.runtime / "runtime.py")))
        self.until(lambda: self.request(url, "/v1/sync/head").get("head_batch") == "1", name)
        return url, process

    def canonical_checks(self):
        codec = self.bin / "layerx-archive-codec"
        self.body1 = self.request(self.origin1, "/v1/sync/batches/1", raw=True)
        command = [codec, "verify", "77", self.pins["sequencer_id"],
                   self.pins["sequencer_public_key"], "1", "18446744073709551615"]
        self.verified1 = json.loads(execute(command, data=self.body1).stdout)
        require(self.verified1["activities"][0]["canonical_hex"] == self.activity0.hex(),
                "origin changed canonical activity bytes")
        require(bool(self.verified1["maintenance"]), "maintenance receipts missing from canonical batch")
        damaged = bytearray(self.body1)
        damaged[-1] ^= 1
        execute(command, data=damaged, success=False)
        wrong_key = list(command)
        wrong_key[4] = "00" * 32
        execute(wrong_key, data=self.body1, success=False)
        wrong_network = list(command)
        wrong_network[2] = "78"
        execute(wrong_network, data=self.body1, success=False)
        execute([codec, "activity"], data=self.activity0[:-1], success=False)
        self.passed("native signature/root verification rejects tampering, foreign trust and truncated activity")

    def relay_config(self, name, upstreams):
        port = unused_port()
        return dict(self.pins, data_dir=str(self.work / name), listen=f"127.0.0.1:{port}",
                    public_url=f"http://127.0.0.1:{port}", upstreams=upstreams,
                    submission_upstreams=[self.origin1 + "/v1/activities", self.origin2 + "/v1/activities"])

    def launch_relay(self, config, name="relay", binary=None, runtime=None):
        path = self.work / (name + ".json")
        write_json(path, config)
        process = self.start(name, [binary or self.bin / "layerxd", "--relay-archive", path],
                             env=dict(os.environ, LAYERX_RELAY_ARCHIVE_RUNTIME=str(runtime or self.runtime / "runtime.py")))
        self.until(lambda: self.request(config["public_url"], "/v1/sync/head").get("head_batch") == "1",
                   name + " backfill")
        return process

    def history_and_forwarding(self):
        self.relay_settings = self.relay_config("relay-data", [self.origin1, self.origin2])
        installed = self.install_operator()
        self.relay_settings["codec"] = str(installed / "layerx-archive-codec")
        self.relay_process = self.launch_relay(self.relay_settings, binary=installed / "layerxd",
                                               runtime=installed / "runtime.py")
        url = self.relay_settings["public_url"]
        detail = self.request(url, "/v1/history/activities/" + self.id0)
        require(detail["canonical_hex"] == self.activity0.hex(), "backfill changed activity bytes")
        require(detail["receipt_hex"] == self.verified1["activities"][0]["receipt_hex"],
                "backfill changed receipt bytes")
        require(self.request(url, "/v1/sync/batches/1", raw=True) == self.body1, "batch storage is not byte exact")
        batch = self.request(url, "/v1/history/batches/1")
        require(len(batch["maintenance"]) == len(self.verified1["maintenance"]),
                "public history omitted maintenance receipts")
        require(batch["maintenance"][0]["receipt_hex"] == self.verified1["maintenance"][0]["receipt_hex"],
                "public maintenance receipt bytes changed")
        self.passed("fresh relay pins genesis and backfills exact public canonical batches and receipts")
        self.activity1 = execute([self.bin / "sign", "1"]).stdout
        ack = self.request(url, "/v1/activities", data=self.activity1, headers=self.headers)
        require(ack["state"] == "acknowledged", "forwarding advertised wrong status")
        self.id1 = ack["activity_id"]
        self.until(lambda: self.request(url, "/v1/sync/head").get("head_batch") == "2", "live batch 2")
        self.stop(self.origin1_process)
        self.activity2 = execute([self.bin / "sign", "2"]).stdout
        ack2 = self.request(url, "/v1/activities", data=self.activity2, headers=self.headers)
        require(ack2["state"] == "acknowledged", "upstream failover did not acknowledge")
        self.id2 = ack2["activity_id"]
        self.until(lambda: self.request(url, "/v1/sync/head").get("head_batch") == "3", "live batch 3 after failover")
        require(self.request(url, "/v1/activities", data=self.activity2, headers=self.headers) == ack2,
                "same activity did not return durable identical acknowledgement")
        conflicting = execute([self.bin / "sign", "2", "1"]).stdout
        self.request(url, "/v1/activities", data=conflicting, headers=self.headers, status=409)
        self.request(url, "/v1/activities", data=self.activity2,
                     headers=dict(self.headers, Authorization="Bearer unauthorized-test-token"), status=401)
        self.request(url, "/v1/activities", data=self.activity2[:-1], headers=self.headers, status=400)
        expired = execute([self.bin / "sign", "3", "1", "expired"]).stdout
        refused = self.request(url, "/v1/activities", data=expired, headers=self.headers, status=422)
        require(refused["state"] == "refused" and refused["error"]["code"] == "native_refusal",
                "definitive native refusal was rewritten as an unknown transport result")
        require(self.request(url, "/v1/activities", data=expired, headers=self.headers, status=422) == refused,
                "definitive native refusal was not retained idempotently")
        self.passed("live synchronization, unchanged signed forwarding and idempotent upstream failover")
        page = self.request(url, "/v1/history/activities?limit=1")
        require(len(page["items"]) == 1 and page["next_cursor"], "bounded history did not paginate")
        seen = []
        while True:
            seen.extend(item["activity_id"] for item in page["items"])
            if not page["next_cursor"]:
                break
            page = self.request(url, "/v1/history/activities?limit=1&cursor=" +
                                urllib.parse.quote(page["next_cursor"], safe=""))
        require(set(seen) == {self.id0, self.id1, self.id2} and len(seen) == 3,
                "history pagination omitted or repeated activities")
        for field, value in (("actor", detail["actor"]), ("module", detail["module"]), ("batch", "1")):
            page = self.request(url, "/v1/history/activities?" + urllib.parse.urlencode({field: value}))
            require(page["items"] and any(item["activity_id"] == self.id0 for item in page["items"]),
                    field + " index omitted matching activity")
        require(bool(detail["accounts"]), "actor account missing from account index")
        page = self.request(url, "/v1/history/activities?account=" + detail["accounts"][0])
        require(any(item["activity_id"] == self.id0 for item in page["items"]), "account index omitted activity")
        self.request(url, "/v1/history/activities?limit=0", status=400)
        self.request(url, "/v1/history/activities?cursor=invalid", status=400)
        self.passed("complete indexed activity history, exact receipt reads, filters and bounded cursor pagination")
        rpc_headers = dict(self.headers, **{"Content-Type": "application/json"})
        rpc = self.request(url, "/rpc", data=json.dumps({"jsonrpc": "2.0", "id": 1,
            "method": "lx_getArchiveHead", "params": []}).encode(), headers=rpc_headers)
        require(rpc["result"]["head_batch"] == "3", "public RPC head disagrees with durable archive")
        rpc = self.request(url, "/rpc", data=json.dumps({"jsonrpc": "2.0", "id": 2,
            "method": "lx_getActivityStatus", "params": [self.id2]}).encode(), headers=rpc_headers)
        require(rpc["result"]["state"] == "included", "archive RPC misrepresented inclusion as execution")
        rpc = self.request(url, "/rpc", data=json.dumps({"jsonrpc": "2.0", "id": 3,
            "method": "lx_sendActivity", "params": [self.activity2.hex(), "finalised"]}).encode(), headers=rpc_headers)
        require("error" in rpc and rpc["error"]["code"] == -32001,
                "acknowledgement silently satisfied finalised commitment")
        self.passed("public RPC history works and never upgrades acknowledgement or inclusion to finality")
        self.stop(self.relay_process)
        path = self.work / "relay-restarted.json"
        write_json(path, self.relay_settings)
        self.relay_process = self.start("relay-restarted", [self.bin / "layerxd", "--relay-archive", path],
            env=dict(os.environ, LAYERX_RELAY_ARCHIVE_RUNTIME=str(self.runtime / "runtime.py")))
        self.until(lambda: self.request(url, "/v1/sync/head").get("head_batch") == "3", "relay restart")
        require(self.request(url, "/v1/activities", data=self.activity2, headers=self.headers) == ack2,
                "restart lost durable forwarding result")
        require(len(self.request(url, "/v1/history/activities")["items"]) == 3, "restart duplicated history")
        self.passed("archive cursor and idempotent forwarding survive process restart without duplicate records")
        wrong = self.relay_config("wrong-network", [self.origin2])
        wrong["genesis_sha256"] = "01" * 32
        path = self.work / "wrong-network.json"
        write_json(path, wrong)
        execute([sys.executable, self.runtime / "runtime.py", "--config", path, "--once"], success=False)
        require(not (self.work / "wrong-network/genesis.manifest").exists(),
                "untrusted genesis committed during failed bootstrap")
        self.passed("independently pinned genesis rejects a mismatched bootstrap origin")

    def storage_checks(self):
        sys.path.insert(0, str(self.runtime))
        from protocol import NativeCodec, IntegrityError, load_config
        from store import ArchiveStore
        settings = self.relay_config("gap-store", [self.origin2])
        path = self.work / "gap-store.json"
        write_json(path, settings)
        config = load_config(path)
        store = ArchiveStore(config)
        codec = NativeCodec(config)
        genesis = codec.genesis(self.manifest, self.snapshot)
        store.initialize_bootstrap(self.manifest.read_bytes(), self.snapshot.read_bytes(), genesis)
        body3 = self.request(self.origin2, "/v1/sync/batches/3", raw=True)
        verified3 = codec.verify(body3)
        try:
            store.ingest_batch(body3, verified3)
        except IntegrityError:
            pass
        else:
            raise AssertionError("fresh store accepted an authenticated batch across a gap")
        require(store.next_batch() == 1, "rejected gap advanced durable cursor")
        require(store.ingest_batch(self.body1, self.verified1), "first canonical batch not stored")
        require(not store.ingest_batch(self.body1, self.verified1), "same batch was appended twice")
        require(store.next_batch() == 2, "atomic batch cursor does not match stored records")
        self.passed("real verified batch gaps roll back atomically; duplicate raw batches do not advance the cursor")

    def install_operator(self):
        bundle = self.work / "bundle"
        bundle.mkdir()
        for name in ("layerxd", "layerx-archive-codec"):
            shutil.copy2(self.bin / name, bundle / name)
        for path in self.runtime.glob("*.py"):
            shutil.copy2(path, bundle / path.name)
        shutil.copy2(self.runtime / "layerx-relay-archive.service", bundle / "layerx-relay-archive.service")
        manifest = bundle / "manifest.sha256"
        manifest.write_text("".join(hashlib.sha256(path.read_bytes()).hexdigest() + "  " + path.name + "\n"
                                    for path in sorted(bundle.iterdir()) if path.is_file()))
        digest = hashlib.sha256(manifest.read_bytes()).hexdigest()
        prefix = self.work / "installed"
        config_path = self.work / "installed-config.json"
        write_json(config_path, self.relay_settings)
        original_config = config_path.read_bytes()
        command = ["bash", self.runtime / "install.sh", "--bundle", bundle,
                   "--manifest-sha256", digest, "--prefix", prefix, "--no-service",
                   "--config", config_path]
        execute(command)
        require(config_path.read_bytes() == original_config, "installer overwrote supplied operator config")
        release = (prefix / "runtime.py").resolve().parent
        for path in bundle.iterdir():
            if path.name != "manifest.sha256":
                require((release / path.name).read_bytes() == path.read_bytes(), "installer payload mismatch")
        bad_prefix = self.work / "bad-install"
        bad = list(command)
        bad[5] = "01" * 32
        bad[7] = bad_prefix
        execute(bad, success=False)
        require(not (bad_prefix / "layerxd").exists(), "installer published untrusted executable")
        execute(command)
        require(config_path.read_bytes() == original_config, "reinstallation changed operator config")
        self.passed("integrity-pinned operator bundle installs real daemon, preserves config and rejects wrong digest")
        return prefix

    def discovery_checks(self):
        settings = self.relay_config("discovered-data", [])
        settings["peer_discovery"] = {"enabled": True, "seeds": [self.origin2],
                                      "allow_loopback_dev": True, "refresh_interval_seconds": 5}
        path = self.work / "discovery.json"
        write_json(path, settings)
        self.start("discovery", [self.bin / "layerxd", "--relay-archive", path],
                   env=dict(os.environ, LAYERX_RELAY_ARCHIVE_RUNTIME=str(self.runtime / "runtime.py")))
        self.until(lambda: self.request(settings["public_url"], "/v1/sync/head").get("head_batch") == "3",
                   "discovery-only bootstrap and backfill")
        public = self.request(self.origin2, "/v1/peers")
        require(public["genesis_sha256"] == self.pins["genesis_sha256"], "peer identity changed trust pin")
        sys.path.insert(0, str(self.runtime))
        from peers import create_discovery
        discovery = create_discovery(settings, settings)
        now = int(time.time())
        discovery._validate_document(public, now)
        bad_documents = [dict(public, genesis_sha256="01" * 32),
                         dict(public, expires_at=now - 1),
                         dict(public, peers=[{"url": "https://10.0.0.1", "expires_at": public["expires_at"]}])]
        for document in bad_documents:
            try:
                discovery._validate_document(document, now)
            except ValueError:
                continue
            raise AssertionError("unsafe or incompatible peer advertisement accepted")
        self.passed("peer-discovered bootstrap and live-compatible history; foreign, expired and private peers refused")

    def run_all(self):
        self.setup()
        self.canonical_checks()
        self.history_and_forwarding()
        self.storage_checks()
        self.discovery_checks()
        write_json(self.work / "result.json", {"status": "passed", "checks": self.checks,
                   "scope": "public relay archive; canonical inclusion, not execution replay or settlement finality"})


if __name__ == "__main__":
    scenario = Scenario()
    try:
        scenario.run_all()
    finally:
        scenario.close()
