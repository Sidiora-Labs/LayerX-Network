"""Render the interop gateway runtime configuration.

Everything this repository can derive is derived here from the vendored
specifications under `interop/specs/vendor` and from the cluster's own
generated identities. What is left is named one deployment variable at a time
and refused by name when it is absent, so the bring-up needs no hand-authored
document. `LAYERX_BETA_INTEROP_MANIFEST_FILE` stays available as an optional
override that wins field by field.

No conformance suite is synthesised: `interop/specs/vendor/CONFORMANCE.md`
records which upstreams publish one.
"""

import argparse
import hashlib
import json
import os
import pathlib
import re
import sys

ADAPTERS = ("x402", "ap2", "ucp", "visa-tap", "fiat")
TRANSPORTS = ("http", "mcp", "a2a")
ADAPTER_FIELDS = (
    "specification",
    "version",
    "specification_sha256",
    "conformance_suite",
    "conformance_vectors",
    "conformance_sha256",
    "evidence_policy",
)
TRANSPORT_FIELDS = ("version", "specification_sha256", "conformance_sha256")
ROOTS = (
    "x402_supported",
    "ap2_keys",
    "ap2_assets",
    "ucp_payment_handler",
    "visa_agents",
    "visa_targets",
    "fiat_providers",
)

VENDORED_DOCUMENT = {
    "x402": "x402/x402-specification-v2.md",
    "ap2": "ap2/specification.md",
    "ucp": "ucp/specification-checkout.html",
    "visa-tap": "visa-tap/README.md",
}
FIAT_DOCUMENT = "docs/wiki/FiatRamps.md"
TRANSPORT_DOCUMENT = {identifier: "x402/transports/%s.md" % identifier for identifier in TRANSPORTS}
TRANSPORT_VERSION = "2"
UCP_REVISION = "2026-04-08"
SPECIFICATION = {
    "x402": "x402",
    "ap2": "ap2",
    "ucp": "ucp-checkout",
    "visa-tap": "visa-tap",
    "fiat": "layerx-fiat-settlement",
}
VERSION = {
    "x402": "2.0.0",
    "ap2": "1.0.0",
    "ucp": UCP_REVISION.replace("-", ""),
    "visa-tap": "1",
    "fiat": "1",
}
EVIDENCE = {
    "x402": "layerx-receipt",
    "ap2": "verified-mandate+layerx-receipt",
    "ucp": "layerx-receipt",
    "visa-tap": "trusted-agent-credential",
    "fiat": "external-settlement+layerx-receipt",
}

OVERRIDE_VARIABLE = "LAYERX_BETA_INTEROP_MANIFEST_FILE"
CONFORMANCE_VARIABLE = {
    identifier: "LAYERX_BETA_INTEROP_CONFORMANCE_%s" % identifier.upper().replace("-", "_")
    for identifier in ADAPTERS
}
TRANSPORT_VARIABLE = {
    identifier: "LAYERX_BETA_INTEROP_CONFORMANCE_%s" % identifier.upper()
    for identifier in TRANSPORTS
}
ROOT_VARIABLE = {root: "LAYERX_BETA_INTEROP_%s" % root.upper() for root in ROOTS}
ROOT_CONTAINER = {
    "x402_supported": dict,
    "ap2_keys": list,
    "ap2_assets": list,
    "ucp_payment_handler": dict,
    "visa_agents": list,
    "visa_targets": list,
    "fiat_providers": list,
}
EXTERNAL_ROOTS = ("ap2_keys", "ap2_assets", "visa_agents", "visa_targets", "fiat_providers")
CONFORMANCE_FORM = "<suite-identifier>,<vector-count>,<suite-sha256>"
SUITE_PATTERN = re.compile(r"^[a-z0-9_-]{1,64}$")
DIGEST_PATTERN = re.compile(r"^[0-9a-f]{64}$")
COUNT_PATTERN = re.compile(r"^[1-9][0-9]{0,11}$")


class Refused(Exception):
    """Raised with one line per input the deployment must still supply or fix."""

    def __init__(self, reasons):
        super().__init__("; ".join(reasons))
        self.reasons = reasons


def repository_root():
    return pathlib.Path(__file__).resolve().parents[3]


def document_digest(path):
    if not path.is_file():
        raise Refused(["%s is missing from this checkout" % path])
    return hashlib.sha256(path.read_bytes()).hexdigest()


def derived(root):
    """Every configuration field this repository can compute for itself."""
    vendor = root / "interop/specs/vendor"
    adapters = {}
    for identifier in ADAPTERS:
        if identifier in VENDORED_DOCUMENT:
            document = vendor / VENDORED_DOCUMENT[identifier]
        else:
            document = root / FIAT_DOCUMENT
        adapters[identifier] = {
            "specification": (SPECIFICATION[identifier], "vendored specification"),
            "version": (VERSION[identifier], "vendored specification"),
            "specification_sha256": (document_digest(document), str(document.relative_to(root))),
            "conformance_suite": (None, CONFORMANCE_VARIABLE[identifier]),
            "conformance_vectors": (None, CONFORMANCE_VARIABLE[identifier]),
            "conformance_sha256": (None, CONFORMANCE_VARIABLE[identifier]),
            "evidence_policy": (EVIDENCE[identifier], "adapter evidence policy"),
        }
    transports = {}
    for identifier in TRANSPORTS:
        document = vendor / TRANSPORT_DOCUMENT[identifier]
        transports[identifier] = {
            "version": (TRANSPORT_VERSION, "vendored transport binding"),
            "specification_sha256": (document_digest(document), str(document.relative_to(root))),
            "conformance_sha256": (None, TRANSPORT_VARIABLE[identifier]),
        }
    return adapters, transports


def conformance_variables(environ, adapters, transports, reasons):
    for identifier in ADAPTERS:
        variable = CONFORMANCE_VARIABLE[identifier]
        declared = environ.get(variable, "").strip()
        if not declared:
            continue
        parts = [part.strip() for part in declared.split(",")]
        if len(parts) != 3:
            reasons.append("%s must be '%s'" % (variable, CONFORMANCE_FORM))
            continue
        suite, count, digest = parts
        adapters[identifier]["conformance_suite"] = (suite, variable)
        adapters[identifier]["conformance_vectors"] = (count, variable)
        adapters[identifier]["conformance_sha256"] = (digest, variable)
    for identifier in TRANSPORTS:
        variable = TRANSPORT_VARIABLE[identifier]
        declared = environ.get(variable, "").strip()
        if declared:
            transports[identifier]["conformance_sha256"] = (declared, variable)


def cluster_roots(environ, network_id, sequencer_public_key, reasons):
    """The trust roots, defaulting to the cluster's own in-cluster identities."""
    roots = {}
    for root in ROOTS:
        variable = ROOT_VARIABLE[root]
        declared = environ.get(variable, "").strip()
        if declared:
            try:
                value = json.loads(declared)
            except ValueError:
                reasons.append("%s must hold JSON" % variable)
                continue
            roots[root] = (value, variable)
            continue
        if root in EXTERNAL_ROOTS:
            continue
        if network_id is None or sequencer_public_key is None:
            roots[root] = (None, "in-cluster default")
            continue
        if root == "x402_supported":
            network = caip2(network_id, reasons)
            if network is None:
                continue
            roots[root] = (
                {
                    "kinds": [{"x402Version": 2, "scheme": "exact", "network": network}],
                    "extensions": [],
                    "signers": {network: ["did:layerx:%s" % sequencer_public_key]},
                },
                "in-cluster default",
            )
        else:
            roots[root] = (
                {
                    "id": "layerx-ucp-handler",
                    "version": UCP_REVISION,
                    "spec": "https://ucp.dev/%s/specification/checkout/" % UCP_REVISION,
                    "schema": "https://ucp.dev/%s/schemas/shopping/checkout.json" % UCP_REVISION,
                },
                "in-cluster default",
            )
    return roots


def caip2(network_id, reasons):
    """The gateway's own CAIP-2 network, from the network identifier it serves."""
    namespace, separator, reference = network_id.partition("-")
    if not separator or not SUITE_PATTERN.match(namespace) or not SUITE_PATTERN.match(reference):
        reasons.append(
            "the interop network identifier %r is not of the form <namespace>-<reference> and no "
            "CAIP-2 network can be derived from it; declare %s instead"
            % (network_id, ROOT_VARIABLE["x402_supported"])
        )
        return None
    return "%s:%s" % (namespace, reference)


def override(path, adapters, transports, roots, reasons):
    """Apply the optional owner manifest field by field over everything above."""
    try:
        document = json.loads(pathlib.Path(path).read_text())
    except (OSError, ValueError) as error:
        raise Refused(["%s=%s could not be read as JSON: %s" % (OVERRIDE_VARIABLE, path, error)])
    if not isinstance(document, dict):
        raise Refused(["%s=%s must hold a JSON object" % (OVERRIDE_VARIABLE, path)])
    unknown = sorted(set(document) - {"adapters", "transports"} - set(ROOTS))
    if unknown:
        reasons.append("%s declares unknown fields: %s" % (OVERRIDE_VARIABLE, ", ".join(unknown)))
    source = "%s=%s" % (OVERRIDE_VARIABLE, path)
    for section, declared, fields in (
        ("adapters", adapters, ADAPTER_FIELDS),
        ("transports", transports, TRANSPORT_FIELDS),
    ):
        overrides = document.get(section, {})
        if not isinstance(overrides, dict):
            reasons.append("%s %s must be an object" % (source, section))
            continue
        for identifier, entry in overrides.items():
            if identifier not in declared:
                reasons.append("%s %s.%s is not a configured entry" % (source, section, identifier))
                continue
            if not isinstance(entry, dict):
                reasons.append("%s %s.%s must be an object" % (source, section, identifier))
                continue
            for field, value in entry.items():
                if field not in fields:
                    reasons.append(
                        "%s %s.%s.%s is not a configured field" % (source, section, identifier, field)
                    )
                    continue
                declared[identifier][field] = (
                    value,
                    "%s %s.%s.%s" % (source, section, identifier, field),
                )
    for root in ROOTS:
        if root in document:
            roots[root] = (document[root], "%s %s" % (source, root))


def validated(adapters, transports, roots, check_only, reasons):
    document = {"adapters": [], "transports": []}
    for identifier in ADAPTERS:
        entry = adapters[identifier]
        suite, suite_source = entry["conformance_suite"]
        count, count_source = entry["conformance_vectors"]
        digest, digest_source = entry["conformance_sha256"]
        if suite is None or count is None or digest is None:
            reasons.append(
                "%s is required: the imported %s conformance suite as '%s' (no upstream suite is "
                "vendorable, interop/specs/vendor/CONFORMANCE.md)"
                % (CONFORMANCE_VARIABLE[identifier], identifier, CONFORMANCE_FORM)
            )
            continue
        document["adapters"].append(
            {
                "id": identifier,
                "specification": label(entry["specification"], reasons),
                "version": label(entry["version"], reasons),
                "specification_sha256": digest32(entry["specification_sha256"], reasons),
                "conformance_suite": suite_identifier(suite, suite_source, reasons),
                "conformance_vectors": vectors(count, count_source, reasons),
                "conformance_sha256": digest32((digest, digest_source), reasons),
                "evidence_policy": label(entry["evidence_policy"], reasons),
            }
        )
    for identifier in TRANSPORTS:
        entry = transports[identifier]
        digest, digest_source = entry["conformance_sha256"]
        if digest is None:
            reasons.append(
                "%s is required: the digest of the imported %s transport conformance suite (no "
                "upstream suite is vendorable, interop/specs/vendor/CONFORMANCE.md)"
                % (TRANSPORT_VARIABLE[identifier], identifier)
            )
            continue
        document["transports"].append(
            {
                "id": identifier,
                "version": label(entry["version"], reasons),
                "specification_sha256": digest32(entry["specification_sha256"], reasons),
                "conformance_sha256": digest32((digest, digest_source), reasons),
            }
        )
    for root in ROOTS:
        value, source = roots.get(root, (None, ROOT_VARIABLE[root]))
        if value is None and check_only and source == "in-cluster default":
            continue
        if value is None:
            reasons.append(
                "%s is required: %s is a counterparty credential this cluster does not hold"
                % (ROOT_VARIABLE[root], root)
            )
            continue
        if not isinstance(value, ROOT_CONTAINER[root]) or not value:
            reasons.append(
                "%s must be a non-empty JSON %s"
                % (source, "object" if ROOT_CONTAINER[root] is dict else "array")
            )
            continue
        document[root] = value
    return document


def label(field, reasons):
    value, source = field
    if not isinstance(value, str) or not value or len(value) > 512:
        reasons.append("%s must be a bounded non-empty string" % source)
        return value
    return value


def suite_identifier(value, source, reasons):
    if not isinstance(value, str) or not SUITE_PATTERN.match(value):
        reasons.append(
            "%s names a conformance suite outside a-z, 0-9, '-' and '_' within 64 bytes" % source
        )
    return value


def vectors(value, source, reasons):
    if isinstance(value, bool) or not isinstance(value, (int, str)):
        reasons.append("%s must count the imported vectors" % source)
        return value
    text = str(value)
    if not COUNT_PATTERN.match(text):
        reasons.append(
            "%s must count the imported vectors as a positive integer; a suite with no vectors is "
            "not a conformance suite" % source
        )
        return value
    return int(text)


def digest32(field, reasons):
    value, source = field
    if not isinstance(value, str) or not DIGEST_PATTERN.match(value):
        reasons.append("%s must be a lowercase 32-byte hexadecimal digest" % source)
        return value
    if int(value, 16) == 0:
        reasons.append("%s must pin real content: a zero digest is not a pin" % source)
    return value


def render(root, environ, network_id=None, sequencer_public_key=None):
    """Return the gateway configuration document or raise `Refused`."""
    reasons = []
    adapters, transports = derived(root)
    conformance_variables(environ, adapters, transports, reasons)
    roots = cluster_roots(environ, network_id, sequencer_public_key, reasons)
    manifest = environ.get(OVERRIDE_VARIABLE, "").strip()
    if manifest:
        override(manifest, adapters, transports, roots, reasons)
    document = validated(adapters, transports, roots, network_id is None, reasons)
    if reasons:
        raise Refused(reasons)
    return document


def sequencer_key(path):
    value = pathlib.Path(path).read_text().strip()
    if not DIGEST_PATTERN.match(value):
        raise Refused(["%s does not hold the generated sequencer public key" % path])
    return value


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--network-id")
    parser.add_argument("--sequencer-public-key-file")
    parser.add_argument("--out")
    parser.add_argument("--repo-root", default=str(repository_root()))
    arguments = parser.parse_args(argv)
    if arguments.self_test:
        return self_test()
    root = pathlib.Path(arguments.repo_root)
    try:
        if arguments.check:
            render(root, os.environ)
            return 0
        if not arguments.network_id or not arguments.sequencer_public_key_file or not arguments.out:
            parser.error("--network-id, --sequencer-public-key-file and --out are required")
        document = render(
            root,
            os.environ,
            arguments.network_id,
            sequencer_key(arguments.sequencer_public_key_file),
        )
    except Refused as refusal:
        sys.stderr.write("the interop gateway configuration was refused:\n")
        for reason in refusal.reasons:
            sys.stderr.write("  - %s\n" % reason)
        return 2
    out = pathlib.Path(arguments.out)
    out.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n")
    out.chmod(0o600)
    return 0


def self_test():
    """Exercise the real render against the vendored documents in this checkout."""
    import tempfile

    root = repository_root()
    vendor = root / "interop/specs/vendor"
    network, key = "layerx-testnet", "ab" * 32
    suites = {
        "x402": "x402-v2-vectors,64,%s" % ("11" * 32),
        "ap2": "ap2-v1-vectors,48,%s" % ("22" * 32),
        "ucp": "ucp-checkout-vectors,32,%s" % ("33" * 32),
        "visa-tap": "visa-tap-vectors,24,%s" % ("44" * 32),
        "fiat": "layerx-fiat-vectors,16,%s" % ("55" * 32),
    }
    complete = {CONFORMANCE_VARIABLE[identifier]: value for identifier, value in suites.items()}
    complete.update(
        {TRANSPORT_VARIABLE[identifier]: "66" * 32 for identifier in TRANSPORTS}
    )
    complete.update(
        {
            ROOT_VARIABLE["ap2_keys"]: json.dumps(
                [{"use_case": "checkout-mandate", "key_id": "k1", "public_key_sec1": "04" + "ab" * 64}]
            ),
            ROOT_VARIABLE["ap2_assets"]: json.dumps([{"principal_digest": "33" * 32}]),
            ROOT_VARIABLE["visa_agents"]: json.dumps([{"key_id": "tap-1"}]),
            ROOT_VARIABLE["visa_targets"]: json.dumps([{"authority": "shop.example"}]),
            ROOT_VARIABLE["fiat_providers"]: json.dumps(
                [{"provider": "example-provider", "public_key_ed25519": "dd" * 32}]
            ),
        }
    )

    def refusal(environ, network_id=network, public_key=key):
        try:
            render(root, environ, network_id, public_key)
        except Refused as refused:
            return refused.reasons
        raise AssertionError("the render accepted %r" % sorted(environ))

    reasons = refusal({})
    for variable in list(CONFORMANCE_VARIABLE.values()) + list(TRANSPORT_VARIABLE.values()):
        assert any(reason.startswith(variable) for reason in reasons), variable
    for root_name in EXTERNAL_ROOTS:
        variable = ROOT_VARIABLE[root_name]
        assert any(reason.startswith(variable) for reason in reasons), variable
    assert not any(reason.startswith(ROOT_VARIABLE["x402_supported"]) for reason in reasons)
    assert not any(reason.startswith(ROOT_VARIABLE["ucp_payment_handler"]) for reason in reasons)
    assert render(root, {**complete}) is not None

    document = render(root, dict(complete), network, key)
    adapters = {entry["id"]: entry for entry in document["adapters"]}
    assert sorted(adapters) == sorted(ADAPTERS)
    provenance = {
        identifier: (vendor / VENDORED_DOCUMENT[identifier]).parent / "PROVENANCE.md"
        for identifier in VENDORED_DOCUMENT
    }
    for identifier, path in provenance.items():
        recorded = set(re.findall(r"[0-9a-f]{64}", path.read_text()))
        assert adapters[identifier]["specification_sha256"] in recorded, identifier
        assert adapters[identifier]["specification_sha256"] == hashlib.sha256(
            (vendor / VENDORED_DOCUMENT[identifier]).read_bytes()
        ).hexdigest()
    assert adapters["fiat"]["specification_sha256"] == hashlib.sha256(
        (root / FIAT_DOCUMENT).read_bytes()
    ).hexdigest()
    assert adapters["x402"]["version"] == "2.0.0" and adapters["ap2"]["version"] == "1.0.0"
    assert adapters["ucp"]["version"] == "20260408" and adapters["visa-tap"]["version"] == "1"
    for identifier, declared in suites.items():
        suite, count, digest = declared.split(",")
        assert adapters[identifier]["conformance_suite"] == suite
        assert adapters[identifier]["conformance_vectors"] == int(count)
        assert adapters[identifier]["conformance_sha256"] == digest
        assert adapters[identifier]["evidence_policy"] == EVIDENCE[identifier]
    transports = {entry["id"]: entry for entry in document["transports"]}
    x402_provenance = (vendor / "x402/PROVENANCE.md").read_text()
    for identifier in TRANSPORTS:
        entry = transports[identifier]
        assert entry["version"] == TRANSPORT_VERSION
        assert entry["specification_sha256"] == hashlib.sha256(
            (vendor / TRANSPORT_DOCUMENT[identifier]).read_bytes()
        ).hexdigest()
        line = [
            row for row in x402_provenance.splitlines() if "transports/%s.md" % identifier in row
        ]
        assert line and entry["specification_sha256"] in line[0], identifier
        assert entry["conformance_sha256"] == "66" * 32
    supported = document["x402_supported"]
    assert supported["kinds"] == [
        {"x402Version": 2, "scheme": "exact", "network": "layerx:testnet"}
    ]
    assert supported["signers"] == {"layerx:testnet": ["did:layerx:%s" % key]}
    handler = document["ucp_payment_handler"]
    assert handler["version"] == UCP_REVISION and len(handler["version"]) == 10
    assert handler["spec"].startswith("https://ucp.dev/%s/" % UCP_REVISION)
    assert handler["schema"] == (
        "https://ucp.dev/%s/schemas/shopping/checkout.json" % UCP_REVISION
    )
    assert document["fiat_providers"][0]["provider"] == "example-provider"

    for broken, expected in (
        ("x402-v2-vectors,0,%s" % ("11" * 32), "not a conformance suite"),
        ("x402-v2-vectors,64,%s" % ("00" * 32), "zero digest is not a pin"),
        ("x402-v2-vectors,64,nothex", "hexadecimal digest"),
        ("X402 Vectors,64,%s" % ("11" * 32), "outside a-z"),
        ("x402-v2-vectors,64", CONFORMANCE_FORM),
    ):
        environ = dict(complete)
        environ[CONFORMANCE_VARIABLE["x402"]] = broken
        reasons = refusal(environ)
        assert any(expected in reason for reason in reasons), (broken, reasons)

    environ = dict(complete)
    environ[ROOT_VARIABLE["visa_agents"]] = "[]"
    assert any("non-empty JSON array" in reason for reason in refusal(environ))
    environ[ROOT_VARIABLE["visa_agents"]] = "{"
    assert any("must hold JSON" in reason for reason in refusal(environ))
    assert any(
        "CAIP-2" in reason
        for reason in refusal(dict(complete), network_id="layerxtestnet")
    )

    with tempfile.TemporaryDirectory() as directory:
        manifest = pathlib.Path(directory) / "manifest.json"
        manifest.write_text(
            json.dumps(
                {
                    "adapters": {"ucp": {"conformance_suite": "owner-ucp-suite", "version": "20260409"}},
                    "transports": {"mcp": {"conformance_sha256": "77" * 32}},
                    "ucp_payment_handler": {
                        "id": "owner-handler",
                        "version": UCP_REVISION,
                        "spec": "https://ucp.dev/%s/specification/checkout/" % UCP_REVISION,
                        "schema": "https://ucp.dev/%s/schemas/shopping/checkout.json" % UCP_REVISION,
                    },
                }
            )
        )
        environ = dict(complete)
        environ[OVERRIDE_VARIABLE] = str(manifest)
        overridden = render(root, environ, network, key)
        adapters = {entry["id"]: entry for entry in overridden["adapters"]}
        assert adapters["ucp"]["conformance_suite"] == "owner-ucp-suite"
        assert adapters["ucp"]["version"] == "20260409"
        assert adapters["ucp"]["specification_sha256"] == document["adapters"][2][
            "specification_sha256"
        ]
        assert adapters["x402"] == {
            key_name: value for key_name, value in document["adapters"][0].items()
        }
        transports = {entry["id"]: entry for entry in overridden["transports"]}
        assert transports["mcp"]["conformance_sha256"] == "77" * 32
        assert transports["http"]["conformance_sha256"] == "66" * 32
        assert overridden["ucp_payment_handler"]["id"] == "owner-handler"
        assert overridden["x402_supported"] == document["x402_supported"]

        manifest.write_text(json.dumps({"adapters": {"ucp": {"conformance_vectors": 0}}}))
        assert any("not a conformance suite" in reason for reason in refusal(environ))
        manifest.write_text(json.dumps({"adapters": {"ucp": {"unknown": 1}}}))
        assert any("is not a configured field" in reason for reason in refusal(environ))
        manifest.write_text(json.dumps({"unexpected": 1}))
        assert any("unknown fields" in reason for reason in refusal(environ))

    sys.stdout.write(
        "interop gateway render: %d adapters and %d transports derived from the vendored "
        "specifications; %d deployment variables refused by name when absent\n"
        % (
            len(ADAPTERS),
            len(TRANSPORTS),
            len(CONFORMANCE_VARIABLE) + len(TRANSPORT_VARIABLE) + len(EXTERNAL_ROOTS),
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
