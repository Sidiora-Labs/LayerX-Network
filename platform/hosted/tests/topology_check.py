#!/usr/bin/env python3
import copy
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[3]
ENGINE_PATH = ROOT / "platform/hosted/tests/topology-check.sh"
engine_source = ENGINE_PATH.read_text().split("<<'PY'\n", 1)[1].split("\nPY\n", 1)[0]
engine = {"__name__": "layerx_topology"}
exec(compile(engine_source.rsplit("sys.exit(main())", 1)[0], str(ENGINE_PATH), "exec"), engine)


def env(container):
    return {entry["name"]: entry.get("value") for entry in container.get("env", [])}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def validate(documents):
    node = next(doc for doc in documents if doc.get("kind") == "StatefulSet" and doc["metadata"]["name"] == "layerx-node")
    pod = node["spec"]["template"]["spec"]
    containers = {container["name"]: container for container in pod["containers"]}
    volumes = {volume["name"]: volume for volume in pod["volumes"]}
    args = containers["layerxd"]["args"]
    allowed_uid = int(args[args.index("--lni-uid") + 1])
    data_paths, tls_secrets = set(), set()
    for number, port, peer_port in [(1, 9451, 9452), (2, 9452, 9451)]:
        name = f"guarantor-{number}"
        require(name in containers, f"missing {name}")
        container = containers[name]
        values = env(container)
        require(container.get("command") == ["/opt/layerx/guarantor.sh"], f"{name} startup entry point")
        require(int(container["securityContext"]["runAsUser"]) == allowed_uid, f"{name} unauthorized LNI uid")
        require(values.get("LAYERX_GUARANTOR_PEER_URL") == f"https://127.0.0.1:{peer_port}", f"{name} peer must be its pod-local counterpart")
        require(values.get("LAYERX_GUARANTOR_LISTEN_PORT") == str(port), f"{name} listener port")
        require(values.get("LAYERX_GUARANTOR_LNI_SOCKET") == "/run/layerx/node/layerxd.lni.sock", f"{name} LNI socket")
        mounts = {mount["mountPath"]: mount for mount in container["volumeMounts"]}
        state_mount = mounts.get("/var/lib/guarantor", {})
        require(state_mount.get("name") == "data" and state_mount.get("subPath") == name, f"{name} isolated persistent state")
        data_paths.add(state_mount["subPath"])
        tls_mount = mounts.get("/run/layerx/guarantor-tls", {})
        require(tls_mount.get("readOnly") in (True, "true"), f"{name} TLS secret must be read only")
        tls_secret = volumes.get(tls_mount.get("name"), {}).get("secret", {}).get("secretName")
        require(tls_secret == f"layerx-{name}-tls", f"{name} distinct TLS identity")
        tls_secrets.add(tls_secret)
        for key, path in [("CA", "ca.crt"), ("CERT", "tls.crt"), ("KEY", "tls.key")]:
            require(values.get(f"LAYERX_GUARANTOR_TLS_{key}_FILE") == f"/run/layerx/guarantor-tls/{path}", f"{name} mTLS {key}")
        submitter = mounts.get("/run/layerx/checkpoint-submitter", {})
        require(submitter.get("readOnly") in (True, "true"), f"{name} submitter secret must be read only")
        require(volumes.get(submitter.get("name"), {}).get("secret", {}).get("secretName") == "paxeer-checkpoint-submitter", f"{name} dedicated funded submitter")
        require(values.get("LAYERX_GUARANTOR_SUBMITTER_KEY_FILE") == "/run/layerx/checkpoint-submitter/key", f"{name} submitter key path")
        lock_mount = mounts.get("/var/lib/guarantor-submitter", {})
        require(lock_mount.get("name") == "data" and lock_mount.get("subPath") == "guarantor-submitter", f"{name} shared submitter nonce lock volume")
        require(values.get("LAYERX_GUARANTOR_SUBMITTER_LOCK_FILE") == "/var/lib/guarantor-submitter/submitter.lock", f"{name} shared submitter nonce lock")
        require(values.get("LAYERX_GUARANTOR_SETTLEMENT_FILE") == "/run/layerx/settlement/checkpoint-settlement.json", f"{name} deployed settlement domain")
        for forbidden in ("/run/layerx/keys", "/var/lib/layerx", "/var/lib/layerx/node"):
            require(forbidden not in mounts, f"{name} must not mount node signing keys")
    require(len(data_paths) == 2 and len(tls_secrets) == 2, "guarantor isolation")
    topology = engine["Topology"]()
    for doc in documents:
        topology.add(doc, str(ROOT / "platform/hosted/node/deployment.yaml"))
    node_workload = next(w for w in topology.workloads if w["name"] == "layerx-node")
    paxeer = next(w for w in topology.workloads if w["name"] == "paxeer")
    for direction, result in [
        ("egress", topology.egress_admits(node_workload, paxeer["ns"], paxeer["labels"], "9443", "TCP", paxeer["ports"])),
        ("ingress", topology.ingress_admits(paxeer, "9443", "TCP", node_workload)),
    ]:
        require(result[0], f"guarantor relay {direction}: {result[1]}")
    for service in topology.services.values():
        if service["selector"].get("app") == "layerx-node":
            require(not any(p["targetPort"] in ("9451", "9452", "guarantor-1-tls", "guarantor-2-tls") for p in service["ports"]), "guarantor exchange must not be exposed by a Service")
    for caller in topology.workloads:
        if caller["name"] == "layerx-node":
            continue
        for port in ("9451", "9452"):
            require(not topology.ingress_admits(node_workload, port, "TCP", caller)[0], f"guarantor exchange exposed to {caller['name']}")


def negative_tests(documents):
    def container(docs, name="guarantor-1"):
        node = next(d for d in docs if d.get("kind") == "StatefulSet" and d["metadata"]["name"] == "layerx-node")
        return next(c for c in node["spec"]["template"]["spec"]["containers"] if c["name"] == name)

    def set_env(docs, key, value):
        next(e for e in container(docs)["env"] if e["name"] == key)["value"] = value

    def share_state(docs):
        next(m for m in container(docs)["volumeMounts"] if m["mountPath"] == "/var/lib/guarantor")["subPath"] = "guarantor-2"

    def remove_relay(docs):
        policy = next(d for d in docs if d.get("kind") == "NetworkPolicy" and d["metadata"]["name"] == "layerx-node-egress")
        policy["spec"]["egress"] = []

    def expose_exchange(docs):
        policy = next(d for d in docs if d.get("kind") == "NetworkPolicy" and d["metadata"]["name"] == "layerx-node-ingress")
        policy["spec"]["ingress"].append({"ports": [{"protocol": "TCP", "port": 9451}]})

    mutations = [
        ("plaintext-peer", lambda docs: set_env(docs, "LAYERX_GUARANTOR_PEER_URL", "http://127.0.0.1:9452")),
        ("missing-client-ca", lambda docs: set_env(docs, "LAYERX_GUARANTOR_TLS_CA_FILE", "")),
        ("unauthorized-lni-uid", lambda docs: container(docs)["securityContext"].update(runAsUser=9999)),
        ("shared-state", share_state),
        ("private-submitter-lock", lambda docs: set_env(docs, "LAYERX_GUARANTOR_SUBMITTER_LOCK_FILE", "/var/lib/guarantor/state/submitter.lock")),
        ("relay-egress-denied", remove_relay),
        ("exchange-ingress-exposed", expose_exchange),
    ]
    for name, mutate in mutations:
        changed = copy.deepcopy(documents)
        mutate(changed)
        try:
            validate(changed)
        except ValueError:
            print(f"guarantor-topology: negative {name} rejected")
        else:
            raise ValueError(f"negative topology case unexpectedly accepted: {name}")


def main():
    _, load = engine["choose_parser"]()
    manifests = ["node", "paxeer", "gateway", "testnet", "identity", "registry", "webhooks"]
    documents = []
    for name in manifests:
        documents.extend(load((ROOT / f"platform/hosted/{name}/deployment.yaml").read_text()))
    validate(documents)
    negative_tests(documents)
    print("guarantor-topology: two mTLS identities, LNI, isolated storage, relay edges and seven negative mutations passed")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, KeyError, StopIteration) as error:
        print(f"guarantor-topology: FAIL {error}", file=sys.stderr)
        sys.exit(1)
