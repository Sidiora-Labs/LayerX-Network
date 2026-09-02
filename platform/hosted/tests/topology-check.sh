#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: topology-check.sh [--yaml-parser auto|pyyaml|builtin] [--strict] [manifest ...]

Parses the hosted Kubernetes manifests, resolves every in-cluster URL that a
workload configures (container env values, ConfigMap-sourced env values and
Ingress backends) to a Service and port, and checks each declared edge against
NetworkPolicy on both ends: the callee's ingress rules and the caller's egress
rules, including the DNS egress the hostname needs. Any host without a Service,
any port the Service does not expose, any targetPort that is not a container
port, any Service that selects no workload and any policy that does not admit
the edge fails the check.

Default manifests:
  platform/hosted/testnet/deployment.yaml
  platform/hosted/gateway/deployment.yaml
  platform/hosted/registry/deployment.yaml

Services that platform/hosted/testnet/README.md names as separately operated
have no manifest in this repository. Their edges are reported as `external`
with the caller's side checked; --strict fails them.
EOF
}

topology_check() {
  local parser=auto strict=0 manifest
  local -a manifests=()
  while [ $# -gt 0 ]; do
    case "$1" in
      --yaml-parser)
        parser="${2:?topology-check: --yaml-parser needs a value}"
        shift 2
        ;;
      --yaml-parser=*)
        parser="${1#*=}"
        shift
        ;;
      --strict)
        strict=1
        shift
        ;;
      -h|--help)
        usage
        return 0
        ;;
      --)
        shift
        manifests+=("$@")
        break
        ;;
      -*)
        printf 'topology-check: unknown option %s\n' "$1" >&2
        usage >&2
        return 2
        ;;
      *)
        manifests+=("$1")
        shift
        ;;
    esac
  done
  case "$parser" in
    auto|pyyaml|builtin) ;;
    *)
      printf 'topology-check: --yaml-parser must be auto, pyyaml or builtin\n' >&2
      return 2
      ;;
  esac
  if [ "${#manifests[@]}" -eq 0 ]; then
    local root
    root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
    manifests=(
      "$root/platform/hosted/testnet/deployment.yaml"
      "$root/platform/hosted/gateway/deployment.yaml"
      "$root/platform/hosted/registry/deployment.yaml"
    )
  fi
  if ! command -v python3 >/dev/null 2>&1; then
    printf 'topology-check: python3 is required\n' >&2
    return 2
  fi
  for manifest in "${manifests[@]}"; do
    if [ ! -r "$manifest" ]; then
      printf 'topology-check: manifest %s is not readable\n' "$manifest" >&2
      return 2
    fi
  done
  TOPOLOGY_YAML_PARSER="$parser" TOPOLOGY_STRICT="$strict" python3 - "${manifests[@]}" <<'PY'
import os
import re
import sys

MODE = os.environ.get("TOPOLOGY_YAML_PARSER", "auto")
STRICT = os.environ.get("TOPOLOGY_STRICT", "0") == "1"
MANIFESTS = sys.argv[1:]

SEPARATELY_OPERATED = {
    ("layerx-testnet", "layerx-pending-core"): {"port": "9443", "labels": {"layerx-plane": "trusted-boundary"}},
    ("layerx-testnet", "layerx-pending-core-admin"): {"port": "9444", "labels": {"layerx-plane": "trusted-boundary"}},
    ("layerx-testnet", "paxeer-boundary"): {"port": "9443", "labels": {"layerx-plane": "trusted-boundary"}},
    ("layerx-testnet", "layerx-identity"): {"port": "9443", "labels": {"layerx-plane": "trusted-boundary"}},
    ("layerx-testnet", "layerx-receipt-authority"): {"port": "9443", "labels": {"layerx-plane": "trusted-boundary"}},
    ("layerx-testnet", "layerx-agent-boundary"): {"port": "9443", "labels": {"layerx-plane": "trusted-boundary"}},
    ("layerx-status", "status-publisher"): {"port": "443", "labels": None},
}
DEFAULT_PORTS = {"https": "443", "http": "80", "rediss": "6379", "redis": "6379"}
URL = re.compile(r"^(https?|rediss?)://([^/:?#\s]+)(?::(\d+))?(?:[/?#].*)?$")
SERVICE_HOST = re.compile(r"^([a-z0-9-]+)\.([a-z0-9-]+)\.svc(?:\.cluster\.local)?\.?$")
INGRESS_CONTROLLER = {"kind": "IngressController", "name": "ingress-nginx", "ns": "ingress-nginx", "labels": None, "ports": {}}


class YamlError(Exception):
    pass


def strip_comment(line):
    quote = None
    for index, char in enumerate(line):
        if quote:
            if char == quote:
                quote = None
        elif char in "\"'":
            quote = char
        elif char == "#" and (index == 0 or line[index - 1] in " \t"):
            return line[:index]
    return line


def unescape(char):
    return {"n": "\n", "t": "\t", "r": "\r", "0": "\0"}.get(char, char)


def plain_scalar(text):
    text = text.strip()
    if text in ("null", "~", ""):
        return None
    return text


class Flow:
    def __init__(self, text):
        self.text = text
        self.index = 0

    def peek(self):
        if self.index >= len(self.text):
            raise YamlError("flow collection is unterminated")
        return self.text[self.index]

    def skip(self):
        while self.index < len(self.text) and self.text[self.index] in " \t\r\n":
            self.index += 1

    def parse(self):
        value = self.value()
        self.skip()
        if self.index != len(self.text):
            raise YamlError("trailing content after flow collection: %r" % self.text[self.index:])
        return value

    def value(self):
        self.skip()
        char = self.peek()
        if char == "{":
            return self.mapping()
        if char == "[":
            return self.sequence()
        if char in "\"'":
            return self.quoted()
        start = self.index
        while self.index < len(self.text) and self.text[self.index] not in ",}]":
            self.index += 1
        return plain_scalar(self.text[start:self.index])

    def key(self):
        self.skip()
        if self.peek() in "\"'":
            return self.quoted()
        start = self.index
        while self.peek() != ":":
            self.index += 1
        return self.text[start:self.index].strip()

    def quoted(self):
        quote = self.peek()
        self.index += 1
        out = []
        while True:
            char = self.peek()
            if quote == '"' and char == "\\":
                self.index += 1
                out.append(unescape(self.peek()))
                self.index += 1
                continue
            if char == quote:
                if quote == "'" and self.index + 1 < len(self.text) and self.text[self.index + 1] == "'":
                    out.append("'")
                    self.index += 2
                    continue
                self.index += 1
                return "".join(out)
            out.append(char)
            self.index += 1

    def mapping(self):
        self.index += 1
        result = {}
        while True:
            self.skip()
            if self.peek() == "}":
                self.index += 1
                return result
            key = self.key()
            self.skip()
            if self.peek() != ":":
                raise YamlError("flow mapping entry without ':'")
            self.index += 1
            if key in result:
                raise YamlError("duplicate key %r" % key)
            result[key] = self.value()
            self.skip()
            if self.peek() == ",":
                self.index += 1
            elif self.peek() != "}":
                raise YamlError("flow mapping entry is not followed by ',' or '}'")

    def sequence(self):
        self.index += 1
        result = []
        while True:
            self.skip()
            if self.peek() == "]":
                self.index += 1
                return result
            result.append(self.value())
            self.skip()
            if self.peek() == ",":
                self.index += 1
            elif self.peek() != "]":
                raise YamlError("flow sequence item is not followed by ',' or ']'")


MAPPING_KEY = re.compile(r"""^("[^"]*"|'[^']*'|[^\s"'{\[\]},][^:]*?)\s*:(?=\s|$)""")
BLOCK_SCALAR = re.compile(r"^[|>][+-]?$")


def unbalanced(text):
    depth = 0
    quote = None
    for char in text:
        if quote:
            if char == quote:
                quote = None
        elif char in "\"'":
            quote = char
        elif char in "{[":
            depth += 1
        elif char in "}]":
            depth -= 1
    return depth > 0


class Chunk:
    def __init__(self, text):
        self.raw = text.splitlines()
        self.pos = 0

    def current(self):
        while self.pos < len(self.raw):
            content = strip_comment(self.raw[self.pos])
            if content.strip():
                indent = len(content) - len(content.lstrip(" "))
                if "\t" in content[:indent]:
                    raise YamlError("line %d uses tab indentation" % (self.pos + 1))
                return indent, content.strip()
            self.pos += 1
        return None

    def parse(self):
        cur = self.current()
        if cur is None:
            return None
        value = self.parse_node(cur[0])
        if self.current() is not None:
            raise YamlError("line %d was not consumed" % (self.pos + 1))
        return value

    def is_item(self, content):
        return content == "-" or content.startswith("- ")

    def parse_node(self, indent):
        cur = self.current()
        if cur is None or cur[0] != indent:
            raise YamlError("line %d: unexpected indentation" % (self.pos + 1))
        if self.is_item(cur[1]):
            return self.parse_sequence(indent)
        return self.parse_mapping(indent)

    def parse_sequence(self, indent):
        items = []
        while True:
            cur = self.current()
            if cur is None or cur[0] != indent or not self.is_item(cur[1]):
                return items
            item = cur[1][1:].strip()
            if item == "":
                self.pos += 1
                nxt = self.current()
                if nxt is None or nxt[0] <= indent:
                    items.append(None)
                else:
                    items.append(self.parse_node(nxt[0]))
            elif item[0] not in "{[\"'" and MAPPING_KEY.match(item):
                inner = indent + (len(cur[1]) - len(item))
                self.raw[self.pos] = " " * inner + item
                items.append(self.parse_mapping(inner))
            else:
                self.pos += 1
                items.append(self.parse_inline(item))

    def parse_mapping(self, indent):
        result = {}
        while True:
            cur = self.current()
            if cur is None or cur[0] != indent or self.is_item(cur[1]):
                return result
            match = MAPPING_KEY.match(cur[1])
            if not match:
                raise YamlError("line %d: expected a mapping entry: %s" % (self.pos + 1, cur[1]))
            key = match.group(1)
            if key[0] in "\"'":
                key = Flow(key).quoted()
            rest = cur[1][match.end():].strip()
            if key in result:
                raise YamlError("line %d: duplicate key %s" % (self.pos + 1, key))
            self.pos += 1
            if rest == "":
                nxt = self.current()
                if nxt is None or nxt[0] < indent:
                    result[key] = None
                elif nxt[0] == indent:
                    result[key] = self.parse_sequence(indent) if self.is_item(nxt[1]) else None
                else:
                    result[key] = self.parse_node(nxt[0])
            elif BLOCK_SCALAR.match(rest):
                result[key] = self.parse_block_scalar(indent)
            elif rest[0] in "{[":
                result[key] = self.parse_flow(rest)
            else:
                result[key] = self.parse_inline(rest)

    def parse_block_scalar(self, indent):
        lines = []
        while self.pos < len(self.raw):
            raw = self.raw[self.pos]
            if raw.strip() == "":
                lines.append("")
                self.pos += 1
                continue
            line_indent = len(raw) - len(raw.lstrip(" "))
            if line_indent <= indent:
                break
            lines.append(raw)
            self.pos += 1
        return "\n".join(lines)

    def parse_flow(self, text):
        while unbalanced(text):
            if self.pos >= len(self.raw):
                raise YamlError("flow collection is unterminated at end of document")
            text += " " + strip_comment(self.raw[self.pos]).strip()
            self.pos += 1
        return Flow(text).parse()

    def parse_inline(self, text):
        if text[0] in "{[":
            return self.parse_flow(text)
        if text[0] in "\"'":
            flow = Flow(text)
            value = flow.quoted()
            flow.skip()
            if flow.index != len(text):
                raise YamlError("trailing content after quoted scalar: %r" % text)
            return value
        return plain_scalar(text)


def load_builtin(text):
    documents = []
    for chunk in re.split(r"(?m)^---[ \t]*$\n?", text):
        value = Chunk(chunk).parse()
        if value is not None:
            documents.append(value)
    return documents


def load_pyyaml(text):
    import yaml

    return [document for document in yaml.safe_load_all(text) if document is not None]


def choose_parser():
    if MODE == "builtin":
        return "builtin", load_builtin
    if MODE == "pyyaml":
        return "pyyaml", load_pyyaml
    try:
        import yaml  # noqa: F401
    except ImportError:
        return "builtin", load_builtin
    return "pyyaml", load_pyyaml


def text(value):
    if value is None:
        return ""
    if value is True:
        return "true"
    if value is False:
        return "false"
    return str(value)


def get(value, *path, default=None):
    for key in path:
        if not isinstance(value, dict) or key not in value or value[key] is None:
            return default
        value = value[key]
    return value


def labels_of(value):
    return {text(key): text(item) for key, item in (value or {}).items()} if isinstance(value, dict) else {}


def label_selector_matches(selector, labels):
    if not isinstance(selector, dict):
        return False
    match_labels = labels_of(get(selector, "matchLabels", default={}))
    expressions = get(selector, "matchExpressions", default=[]) or []
    if not match_labels and not expressions:
        return True
    if labels is None:
        return False
    for key, value in match_labels.items():
        if labels.get(key) != value:
            return False
    for expression in expressions:
        key = text(get(expression, "key"))
        operator = text(get(expression, "operator"))
        values = [text(item) for item in (get(expression, "values", default=[]) or [])]
        present = key in labels
        if operator == "In" and not (present and labels[key] in values):
            return False
        if operator == "NotIn" and present and labels[key] in values:
            return False
        if operator == "Exists" and not present:
            return False
        if operator == "DoesNotExist" and present:
            return False
        if operator not in ("In", "NotIn", "Exists", "DoesNotExist"):
            return False
    return True


class Topology:
    def __init__(self):
        self.namespaces = {}
        self.services = {}
        self.configmaps = {}
        self.workloads = []
        self.policies = []
        self.ingresses = []
        self.problems = []
        self.notes = []

    def add(self, document, source):
        kind = text(get(document, "kind"))
        name = text(get(document, "metadata", "name"))
        ns = text(get(document, "metadata", "namespace")) or "default"
        if kind == "Namespace":
            labels = labels_of(get(document, "metadata", "labels", default={}))
            labels.setdefault("kubernetes.io/metadata.name", name)
            self.namespaces[name] = labels
        elif kind == "Service":
            ports = []
            for port in get(document, "spec", "ports", default=[]) or []:
                ports.append({
                    "name": text(get(port, "name")),
                    "port": text(get(port, "port")),
                    "targetPort": text(get(port, "targetPort")) or text(get(port, "port")),
                    "protocol": text(get(port, "protocol")) or "TCP",
                })
            self.services[(ns, name)] = {
                "selector": labels_of(get(document, "spec", "selector", default={})),
                "ports": ports,
                "source": source,
            }
        elif kind == "ConfigMap":
            self.configmaps[(ns, name)] = {text(key): text(value) for key, value in (get(document, "data", default={}) or {}).items()}
        elif kind in ("Deployment", "StatefulSet", "DaemonSet", "Job", "ReplicaSet"):
            self.add_workload(kind, name, ns, get(document, "spec", "template", default={}), source)
        elif kind == "CronJob":
            self.add_workload(kind, name, ns, get(document, "spec", "jobTemplate", "spec", "template", default={}), source)
        elif kind == "NetworkPolicy":
            spec = get(document, "spec", default={}) or {}
            types = [text(item) for item in (get(spec, "policyTypes") or [])]
            if not types:
                types = ["Ingress"] + (["Egress"] if "egress" in spec else [])
            self.policies.append({
                "name": name,
                "ns": ns,
                "selector": get(spec, "podSelector", default={}) or {},
                "types": types,
                "ingress": get(spec, "ingress", default=[]) or [],
                "egress": get(spec, "egress", default=[]) or [],
            })
        elif kind == "Ingress":
            backends = []
            default = get(document, "spec", "defaultBackend", "service")
            if default:
                backends.append(("(default backend)", text(get(default, "name")), text(get(default, "port", "name")) or text(get(default, "port", "number"))))
            for rule in get(document, "spec", "rules", default=[]) or []:
                host = text(get(rule, "host")) or "*"
                for path in get(rule, "http", "paths", default=[]) or []:
                    service = get(path, "backend", "service", default={}) or {}
                    backends.append((host + text(get(path, "path")), text(get(service, "name")), text(get(service, "port", "name")) or text(get(service, "port", "number"))))
            self.ingresses.append({"name": name, "ns": ns, "backends": backends})

    def add_workload(self, kind, name, ns, template, source):
        labels = labels_of(get(template, "metadata", "labels", default={}))
        ports = {}
        env = []
        containers = list(get(template, "spec", "containers", default=[]) or []) + list(get(template, "spec", "initContainers", default=[]) or [])
        for container in containers:
            for port in get(container, "ports", default=[]) or []:
                port_name = text(get(port, "name"))
                if port_name:
                    ports[port_name] = (text(get(port, "containerPort")), text(get(port, "protocol")) or "TCP")
            for entry in get(container, "env", default=[]) or []:
                env_name = text(get(entry, "name"))
                if get(entry, "value") is not None:
                    env.append((env_name, text(get(entry, "value"))))
                    continue
                reference = get(entry, "valueFrom", "configMapKeyRef")
                if reference:
                    env.append((env_name, ("configmap", text(get(reference, "name")), text(get(reference, "key")))))
            for entry in get(container, "envFrom", default=[]) or []:
                reference = get(entry, "configMapRef")
                if reference:
                    env.append((None, ("configmap-all", text(get(reference, "name")), text(get(entry, "prefix")))))
        self.workloads.append({"kind": kind, "name": name, "ns": ns, "labels": labels, "ports": ports, "env": env, "source": source})

    def resolved_env(self, workload):
        for env_name, value in workload["env"]:
            if isinstance(value, tuple) and value[0] == "configmap":
                data = self.configmaps.get((workload["ns"], value[1]))
                if data is None:
                    self.notes.append("%s %s/%s env %s comes from ConfigMap %s which is provisioned outside these manifests; its value is not checked" % (workload["kind"], workload["ns"], workload["name"], env_name, value[1]))
                    continue
                if value[2] not in data:
                    self.problems.append("%s %s/%s env %s references key %s missing from ConfigMap %s" % (workload["kind"], workload["ns"], workload["name"], env_name, value[2], value[1]))
                    continue
                yield "env %s (ConfigMap %s/%s)" % (env_name, value[1], value[2]), data[value[2]]
            elif isinstance(value, tuple) and value[0] == "configmap-all":
                data = self.configmaps.get((workload["ns"], value[1]))
                if data is None:
                    self.notes.append("%s %s/%s envFrom ConfigMap %s is provisioned outside these manifests; its values are not checked" % (workload["kind"], workload["ns"], workload["name"], value[1]))
                    continue
                for key, item in data.items():
                    yield "env %s%s (ConfigMap %s)" % (value[2], key, value[1]), item
            else:
                yield "env %s" % env_name, value

    def edges(self):
        for workload in self.workloads:
            for source, value in self.resolved_env(workload):
                match = URL.match(value.strip())
                if not match:
                    continue
                scheme, host, port = match.group(1), match.group(2), match.group(3)
                service_match = SERVICE_HOST.match(host)
                if not service_match:
                    yield {"caller": workload, "source": source, "host": host, "kind": "offcluster", "port": port or DEFAULT_PORTS[scheme]}
                    continue
                yield {
                    "caller": workload,
                    "source": source,
                    "host": host,
                    "kind": "service",
                    "service": (service_match.group(2), service_match.group(1)),
                    "port": port or DEFAULT_PORTS[scheme],
                }
        for ingress in self.ingresses:
            for host_path, service, port in ingress["backends"]:
                yield {
                    "caller": INGRESS_CONTROLLER,
                    "source": "Ingress %s/%s %s" % (ingress["ns"], ingress["name"], host_path),
                    "host": "%s.%s.svc" % (service, ingress["ns"]),
                    "kind": "service",
                    "service": (ingress["ns"], service),
                    "port": port,
                }

    def selected_policies(self, ns, labels, direction):
        return [policy for policy in self.policies if policy["ns"] == ns and direction in policy["types"] and label_selector_matches(policy["selector"], labels)]

    def peer_matches(self, peer, policy_ns, peer_ns, peer_labels):
        block = get(peer, "ipBlock")
        if block is not None:
            cidr = text(get(block, "cidr"))
            return cidr in ("0.0.0.0/0", "::/0") and not (get(block, "except") or [])
        namespace_selector = get(peer, "namespaceSelector")
        pod_selector = get(peer, "podSelector")
        if namespace_selector is None and pod_selector is None:
            return False
        if namespace_selector is None:
            ns_ok = peer_ns == policy_ns
        else:
            ns_labels = dict(self.namespaces.get(peer_ns, {}))
            ns_labels.setdefault("kubernetes.io/metadata.name", peer_ns)
            ns_ok = label_selector_matches(namespace_selector, ns_labels)
        pod_ok = True if pod_selector is None else label_selector_matches(pod_selector, peer_labels)
        return ns_ok and pod_ok

    def port_matches(self, rule_ports, protocol, pod_port, pod_port_names):
        if not rule_ports:
            return True
        for entry in rule_ports:
            if (text(get(entry, "protocol")) or "TCP") != protocol:
                continue
            port = text(get(entry, "port"))
            if port == "":
                return True
            if port.isdigit():
                end = text(get(entry, "endPort"))
                if port == pod_port or (end.isdigit() and pod_port.isdigit() and int(port) <= int(pod_port) <= int(end)):
                    return True
            elif pod_port_names.get(port, (None, None))[0] == pod_port:
                return True
        return False

    def ingress_admits(self, callee, pod_port, protocol, caller):
        policies = self.selected_policies(callee["ns"], callee["labels"], "Ingress")
        if not policies:
            return True, "no ingress NetworkPolicy selects the callee (open)"
        for policy in policies:
            for rule in policy["ingress"]:
                peers = get(rule, "from") or []
                if (not peers or any(self.peer_matches(peer, policy["ns"], caller["ns"], caller["labels"]) for peer in peers)) and self.port_matches(get(rule, "ports") or [], protocol, pod_port, callee["ports"]):
                    return True, "ingress admitted by %s" % policy["name"]
        return False, "ingress NetworkPolicy %s does not admit %s on %s/%s" % (", ".join(policy["name"] for policy in policies), describe(caller), pod_port, protocol)

    def egress_admits(self, caller, callee_ns, callee_labels, pod_port, protocol, pod_port_names):
        if caller["labels"] is None:
            return True, "caller egress is outside these manifests"
        policies = self.selected_policies(caller["ns"], caller["labels"], "Egress")
        if not policies:
            return True, "no egress NetworkPolicy selects the caller (open)"
        admitted = None
        dns = None
        for policy in policies:
            for rule in policy["egress"]:
                peers = get(rule, "to") or []
                peer_ok = not peers or any(self.peer_matches(peer, policy["ns"], callee_ns, callee_labels) for peer in peers)
                if peer_ok and self.port_matches(get(rule, "ports") or [], protocol, pod_port, pod_port_names):
                    admitted = admitted or policy["name"]
                dns_peer_ok = not peers or any(get(peer, "namespaceSelector") is not None and label_selector_matches(get(peer, "namespaceSelector"), {"kubernetes.io/metadata.name": "kube-system"}) and get(peer, "podSelector") is None for peer in peers)
                if dns_peer_ok and self.port_matches(get(rule, "ports") or [], "UDP", "53", {}):
                    dns = dns or policy["name"]
        if admitted is None:
            return False, "egress NetworkPolicy %s does not admit %s/%s to the callee" % (", ".join(policy["name"] for policy in policies), pod_port, protocol)
        if dns is None:
            return False, "egress NetworkPolicy %s does not admit DNS (UDP 53 to every namespace) so the hostname cannot resolve" % ", ".join(policy["name"] for policy in policies)
        return True, "egress admitted by %s (DNS via %s)" % (admitted, dns)


def describe(workload):
    return "%s %s/%s" % (workload["kind"], workload["ns"], workload["name"])


def check(topology):
    results = []
    for edge in topology.edges():
        caller = edge["caller"]
        label = "%s -> %s:%s [%s]" % (describe(caller), edge["host"], edge["port"], edge["source"])
        if edge["kind"] == "offcluster":
            results.append(("note", label, "host is not an in-cluster Service name; not checked"))
            continue
        ns, name = edge["service"]
        service = topology.services.get((ns, name))
        if service is None:
            external = SEPARATELY_OPERATED.get((ns, name))
            if external is None:
                results.append(("FAIL", label, "no Service %s/%s is declared in the manifests and it is not a declared separately operated service" % (ns, name)))
                continue
            if edge["port"] != external["port"]:
                results.append(("FAIL", label, "separately operated service %s/%s is declared on port %s, not %s" % (ns, name, external["port"], edge["port"])))
                continue
            ok, detail = topology.egress_admits(caller, ns, external["labels"], external["port"], "TCP", {})
            if not ok:
                results.append(("FAIL", label, detail))
                continue
            results.append(("external", label, "separately operated; %s; callee policy is not in this repository" % detail))
            continue
        ports = [port for port in service["ports"] if edge["port"] in (port["port"], port["name"])]
        if not ports:
            results.append(("FAIL", label, "Service %s/%s exposes %s, not %s" % (ns, name, ", ".join("%s(%s)" % (port["port"], port["name"] or "-") for port in service["ports"]), edge["port"])))
            continue
        port = ports[0]
        selected = [workload for workload in topology.workloads if workload["ns"] == ns and service["selector"] and all(workload["labels"].get(key) == value for key, value in service["selector"].items())]
        if not selected:
            results.append(("FAIL", label, "Service %s/%s selects no workload declared in the manifests (selector %s)" % (ns, name, service["selector"])))
            continue
        failed = False
        details = []
        for callee in selected:
            if port["targetPort"].isdigit():
                pod_port = port["targetPort"]
                protocol = port["protocol"]
            elif port["targetPort"] in callee["ports"]:
                pod_port, protocol = callee["ports"][port["targetPort"]]
            else:
                results.append(("FAIL", label, "Service %s/%s targetPort %s is not a container port of %s" % (ns, name, port["targetPort"], describe(callee))))
                failed = True
                continue
            ok, detail = topology.ingress_admits(callee, pod_port, protocol, caller)
            if not ok:
                results.append(("FAIL", label, detail))
                failed = True
                continue
            details.append("pod %s:%s/%s %s" % (describe(callee), pod_port, protocol, detail))
            ok, detail = topology.egress_admits(caller, ns, callee["labels"], pod_port, protocol, callee["ports"])
            if not ok:
                results.append(("FAIL", label, detail))
                failed = True
                continue
            details.append(detail)
        if not failed:
            results.append(("ok", label, "; ".join(details)))
    return results


def main():
    parser_name, load = choose_parser()
    topology = Topology()
    print("topology-check: parser=%s manifests=%s" % (parser_name, " ".join(MANIFESTS)))
    for manifest in MANIFESTS:
        with open(manifest, encoding="utf-8") as handle:
            content = handle.read()
        try:
            documents = load(content)
        except YamlError as error:
            print("topology-check: FAIL %s: %s" % (manifest, error))
            return 1
        for document in documents:
            if not isinstance(document, dict):
                print("topology-check: FAIL %s: document is not a mapping" % manifest)
                return 1
            topology.add(document, manifest)
    results = check(topology)
    for note in topology.notes:
        results.append(("note", note, "not checked"))
    for problem in topology.problems:
        results.append(("FAIL", problem, "unresolvable configuration"))
    counts = {"ok": 0, "external": 0, "note": 0, "FAIL": 0}
    for status, label, detail in results:
        counts[status] += 1
        print("%-8s %s: %s" % (status, label, detail))
    print("topology-check: %d edges, ok=%d external=%d note=%d failed=%d" % (counts["ok"] + counts["external"] + counts["FAIL"], counts["ok"], counts["external"], counts["note"], counts["FAIL"]))
    if counts["FAIL"]:
        return 1
    if STRICT and counts["external"]:
        print("topology-check: --strict refuses %d separately operated edge(s)" % counts["external"])
        return 1
    if not counts["ok"] and not counts["external"]:
        print("topology-check: no edges were found; refusing to pass an empty topology")
        return 1
    return 0


sys.exit(main())
PY
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  topology_check "$@"
fi
