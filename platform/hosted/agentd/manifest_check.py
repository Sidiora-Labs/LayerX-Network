#!/usr/bin/env python3
import copy
import pathlib
import re
import sys

import yaml

ROOT = pathlib.Path(__file__).resolve().parents[3]
NODE_MANIFEST = ROOT / "platform/hosted/node/deployment.yaml"
AGENT_MATERIAL = ROOT / "platform/hosted/human/material.py"
NAMESPACE = "layerx-testnet"
WORKLOAD = "layerx-node"
OWNER = "human-owner"
BOUNDARY = "agentd-boundary"
SERVICE = "layerx-agentd"
POLICY = "layerx-node-ingress"
TLS_VOLUME = "agentd-tls"
TLS_MOUNT = "/run/layerx/agentd-tls"
TRUST_MOUNT = "/run/layerx/trust"
ENDPOINT = re.compile(r"(?:127\.0\.0\.1|0\.0\.0\.0|localhost):(\d+)")
AGENT_KEYS = ("MODE", "HUMAN_SOCKET", "PROGRAM_LISTEN")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def agent_config():
    source = AGENT_MATERIAL.read_text(encoding="utf-8")
    start = source.index("\n    agent = {")
    block = source[start:source.index("\n    }\n", start)]
    values = {}
    for key in AGENT_KEYS:
        found = re.search(r"'%s':\s*'([^']*)'" % key, block)
        require(found is not None, f"{AGENT_MATERIAL.name} does not define the agent {key}")
        values[key] = found.group(1)
    return values


def documents():
    return [
        document
        for document in yaml.safe_load_all(NODE_MANIFEST.read_text(encoding="utf-8"))
        if isinstance(document, dict)
    ]


def named(docs, kind, name):
    for document in docs:
        if document.get("kind") == kind and document.get("metadata", {}).get("name") == name:
            return document
    raise ValueError(f"no {kind} {name} is declared in {NODE_MANIFEST.name}")


def env_of(container):
    return {entry["name"]: entry.get("value") for entry in container.get("env", [])}


EXPANSION = re.compile(r"\$\{([A-Za-z_][A-Za-z0-9_]*)[^}]*\}|\$([A-Za-z_][A-Za-z0-9_]*)")


def text_of(container):
    environment = env_of(container)
    parts = list(container.get("command", [])) + list(container.get("args", []))
    parts += [value for value in environment.values() if value is not None]
    for probe in ("readinessProbe", "livenessProbe", "startupProbe"):
        step = container.get(probe, {}).get("exec", {}).get("command", [])
        parts += list(step)
    joined = "\n".join(str(part) for part in parts)
    return EXPANSION.sub(lambda found: str(environment.get(found.group(1) or found.group(2), found.group(0))), joined)


def endpoints(containers):
    claimed = {}
    for name, container in containers.items():
        ports = {str(port) for port in ENDPOINT.findall(text_of(container))}
        ports |= {str(port.get("containerPort")) for port in container.get("ports", [])}
        for port in ports:
            claimed.setdefault(port, set()).add(name)
    return claimed


def listen_port(value, message):
    host, _, port = value.rpartition(":")
    require(host == "127.0.0.1", message)
    require(port.isdigit(), message)
    return port


def validate(docs, config):
    require(config["MODE"] == "human-owner", "the hosted agent must run the human owner mode")
    socket = config["HUMAN_SOCKET"]
    require(socket.startswith("/run/layerx/human/owner/"), "the agent owner socket must stay in the owner-only run directory")
    health = listen_port(config["PROGRAM_LISTEN"], "the agent health listener must bind loopback")

    node = named(docs, "StatefulSet", WORKLOAD)
    pod = node["spec"]["template"]["spec"]
    containers = {container["name"]: container for container in pod["containers"]}
    volumes = {volume["name"]: volume for volume in pod["volumes"]}
    require(OWNER in containers, f"the node pod does not run the {OWNER} container")
    require(BOUNDARY in containers, f"the node pod does not run the {BOUNDARY} container")

    owner = containers[OWNER]
    require(owner.get("command") == ["/usr/local/bin/human-entrypoint", "agent"], "the owner container must start agentd through the human entry point")
    sources = [entry.get("secretRef", {}).get("name") for entry in owner.get("envFrom", [])]
    require("layerx-human-agent-config" in sources, "the owner container must take its configuration from the agent config secret")
    probe = "\n".join(owner["readinessProbe"]["exec"]["command"])
    require(f"http://127.0.0.1:{health}/healthz" in probe, "the owner readiness probe must query the configured loopback health listener")
    require("--config /run/human-private/agent/probe.conf" in probe, "the owner readiness probe must present the agent program bearer")

    boundary = containers[BOUNDARY]
    values = env_of(boundary)
    published = str(boundary["ports"][0]["containerPort"])
    require(len(boundary["ports"]) == 1 and boundary["ports"][0]["name"] == "agentd-tls", "the boundary must publish exactly one named port")
    require(values.get("LAYERX_AGENTD_HEALTH_PORT") == health, "the boundary must forward to the configured agent health listener")
    require(values.get("LAYERX_AGENTD_BOUNDARY_PORT") == published, "the boundary listener must match its published container port")
    relay = "\n".join(str(argument) for argument in boundary["args"])
    require(socket not in text_of(boundary), "the boundary must never reach the agent owner socket")
    require("UNIX-CONNECT" not in relay and "UNIX:" not in relay, "the boundary must never relay to a unix socket")
    require(f'"TCP:127.0.0.1:${{LAYERX_AGENTD_HEALTH_PORT}}"' in relay, "the boundary must forward to loopback only")
    require("OPENSSL-LISTEN:${LAYERX_AGENTD_BOUNDARY_PORT}" in relay, "the boundary must terminate TLS on its published port")
    require("verify=1" in relay, "the boundary must require a client certificate")
    require(f"cafile={TRUST_MOUNT}/ca.crt" in relay, "the boundary must verify clients against the internal CA")
    require(f"cert={TLS_MOUNT}/tls.crt" in relay and f"key={TLS_MOUNT}/tls.key" in relay, "the boundary must serve the agentd server identity")
    security = boundary["securityContext"]
    require(int(security["runAsUser"]) == 4021, "the boundary must run as the owner identity")
    require(security["readOnlyRootFilesystem"] in (True, "true"), "the boundary root filesystem must be read only")
    require(security["allowPrivilegeEscalation"] in (False, "false"), "the boundary must not allow privilege escalation")
    require(security["capabilities"]["drop"] == ["ALL"], "the boundary must drop every capability")
    mounts = {mount["mountPath"]: mount for mount in boundary["volumeMounts"]}
    require(set(mounts) == {TLS_MOUNT, TRUST_MOUNT}, "the boundary must mount its own identity and the trust root and nothing else")
    for path in (TLS_MOUNT, TRUST_MOUNT):
        require(mounts[path].get("readOnly") in (True, "true"), f"the boundary must mount {path} read only")
    require(mounts[TLS_MOUNT]["name"] == TLS_VOLUME, "the boundary server identity must come from the agentd TLS volume")
    require(volumes.get(TLS_VOLUME, {}).get("secret", {}).get("secretName") == "layerx-agentd-tls", "the agentd TLS volume must come from the agentd TLS secret")

    claimed = endpoints(containers)
    require(claimed.get(health) == {OWNER, BOUNDARY}, f"port {health} is claimed by {sorted(claimed.get(health, ()))}, not by the agent health listener and its boundary alone")
    require(claimed.get(published) == {BOUNDARY}, f"port {published} is claimed by {sorted(claimed.get(published, ()))}, not by the boundary alone")

    service = named(docs, "Service", SERVICE)
    require(service["metadata"]["namespace"] == NAMESPACE, "the agentd Service must live beside the node")
    require(service["spec"]["selector"] == {"app": WORKLOAD}, "the agentd Service must select the node pod that runs agentd")
    ports = service["spec"]["ports"]
    require(len(ports) == 1, "the agentd Service must publish exactly one port")
    require(str(ports[0]["targetPort"]) == "agentd-tls", "the agentd Service must publish the boundary port by name")
    for other in docs:
        if other.get("kind") != "Service" or other["spec"].get("selector", {}).get("app") != WORKLOAD:
            continue
        exposed = {str(port.get("targetPort")) for port in other["spec"]["ports"]}
        require(health not in exposed, f"Service {other['metadata']['name']} exposes the agent loopback health listener")

    isolated = False
    for policy in docs:
        if policy.get("kind") != "NetworkPolicy":
            continue
        spec = policy["spec"]
        if spec.get("podSelector") != {"matchLabels": {"app": WORKLOAD}} or "Ingress" not in spec.get("policyTypes", []):
            continue
        isolated = True
        for rule in spec.get("ingress", []):
            ports = {str(port.get("port")) for port in rule.get("ports", [])}
            require(ports, f"NetworkPolicy {policy['metadata']['name']} admits every port of the node pod")
            require(published not in ports, f"NetworkPolicy {policy['metadata']['name']} admits the agentd boundary to a cluster workload")
            require(health not in ports, f"NetworkPolicy {policy['metadata']['name']} admits the agent loopback health listener")
    require(isolated, "the node pod must carry an ingress policy, so the agentd boundary is closed to the cluster by default")


def negative_tests(docs, config):
    def boundary(bundle):
        node = next(d for d in bundle if d.get("kind") == "StatefulSet" and d["metadata"]["name"] == WORKLOAD)
        return next(c for c in node["spec"]["template"]["spec"]["containers"] if c["name"] == BOUNDARY)

    def owner(bundle):
        node = next(d for d in bundle if d.get("kind") == "StatefulSet" and d["metadata"]["name"] == WORKLOAD)
        return next(c for c in node["spec"]["template"]["spec"]["containers"] if c["name"] == OWNER)

    def relay(bundle, old, new):
        container = boundary(bundle)
        container["args"] = [str(argument).replace(old, new) for argument in container["args"]]

    def relay_to_owner_socket(bundle, values):
        container = boundary(bundle)
        container["env"].append({"name": "LAYERX_AGENTD_OWNER_SOCKET", "value": values["HUMAN_SOCKET"]})
        container["args"] = list(container["args"]) + ['socat -T 30 "TCP-LISTEN:9455,fork" "UNIX-CONNECT:${LAYERX_AGENTD_OWNER_SOCKET}"']

    def mount_the_owner_run_directory(bundle, values):
        boundary(bundle)["volumeMounts"].append({"name": "run", "mountPath": "/run/layerx", "readOnly": True})

    def collide(bundle, values):
        values["PROGRAM_LISTEN"] = "127.0.0.1:9451"
        for entry in boundary(bundle)["env"]:
            if entry["name"] == "LAYERX_AGENTD_HEALTH_PORT":
                entry["value"] = "9451"
        probe = owner(bundle)["readinessProbe"]["exec"]["command"]
        probe[:] = [step.replace("127.0.0.1:9453", "127.0.0.1:9451") for step in probe]

    def unprotected_probe(bundle, values):
        probe = owner(bundle)["readinessProbe"]["exec"]["command"]
        probe[:] = [step.replace("--config /run/human-private/agent/probe.conf ", "") for step in probe]

    def service_exposes_health(bundle, values):
        service = next(d for d in bundle if d.get("kind") == "Service" and d["metadata"]["name"] == SERVICE)
        service["spec"]["ports"][0]["targetPort"] = 9453

    def service_selects_nothing(bundle, values):
        service = next(d for d in bundle if d.get("kind") == "Service" and d["metadata"]["name"] == SERVICE)
        service["spec"]["selector"] = {"app": "layerx-elsewhere"}

    def policy_admits_the_boundary(bundle, values):
        policy = next(d for d in bundle if d.get("kind") == "NetworkPolicy" and d["metadata"]["name"] == POLICY)
        policy["spec"]["ingress"].append({"from": [{"podSelector": {"matchLabels": {"app": "layerx-gateway"}}}], "ports": [{"protocol": "TCP", "port": 9454}]})

    def policy_admits_every_port(bundle, values):
        policy = next(d for d in bundle if d.get("kind") == "NetworkPolicy" and d["metadata"]["name"] == POLICY)
        policy["spec"]["ingress"].append({"from": [{"podSelector": {"matchLabels": {"app": "layerx-gateway"}}}]})

    def drop_ingress_isolation(bundle, values):
        policy = next(d for d in bundle if d.get("kind") == "NetworkPolicy" and d["metadata"]["name"] == POLICY)
        bundle.remove(policy)

    mutations = [
        ("health-port-collides-with-the-guarantor-exchange", collide),
        ("boundary-forwards-beyond-loopback", lambda bundle, values: relay(bundle, "TCP:127.0.0.1:", "TCP:0.0.0.0:")),
        ("boundary-accepts-unverified-clients", lambda bundle, values: relay(bundle, ",verify=1", "")),
        ("boundary-trusts-a-foreign-authority", lambda bundle, values: relay(bundle, f"cafile={TRUST_MOUNT}/ca.crt", "cafile=/tmp/ca.crt")),
        ("boundary-root-filesystem-writable", lambda bundle, values: boundary(bundle)["securityContext"].update(readOnlyRootFilesystem=False)),
        ("owner-probe-drops-the-bearer", unprotected_probe),
        ("service-exposes-the-loopback-health-listener", service_exposes_health),
        ("service-selects-no-agentd-workload", service_selects_nothing),
        ("policy-admits-the-agentd-boundary", policy_admits_the_boundary),
        ("policy-admits-every-node-port", policy_admits_every_port),
        ("node-pod-loses-ingress-isolation", drop_ingress_isolation),
        ("boundary-relays-the-owner-socket", relay_to_owner_socket),
        ("boundary-mounts-the-owner-run-directory", mount_the_owner_run_directory),
    ]
    for name, mutate in mutations:
        bundle = copy.deepcopy(docs)
        values = dict(config)
        mutate(bundle, values)
        try:
            validate(bundle, values)
        except (ValueError, KeyError, IndexError, StopIteration):
            print(f"agentd-manifest: negative {name} rejected")
        else:
            raise ValueError(f"negative agentd manifest case unexpectedly accepted: {name}")
    return len(mutations)


def main():
    config = agent_config()
    docs = documents()
    validate(docs, config)
    rejected = negative_tests(docs, config)
    print(
        "agentd-manifest: %s Service, mutually authenticated boundary on the %s loopback health listener,"
        " owner socket %s held private, and %d negative mutations passed"
        % (SERVICE, config["PROGRAM_LISTEN"], config["HUMAN_SOCKET"], rejected)
    )


if __name__ == "__main__":
    try:
        main()
    except (ValueError, KeyError, IndexError, StopIteration, OSError) as error:
        print(f"agentd-manifest: FAIL {error}", file=sys.stderr)
        sys.exit(1)
