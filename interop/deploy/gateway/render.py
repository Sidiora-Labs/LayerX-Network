"""Render the interop gateway runtime configuration.

Everything this repository can derive is derived here from the vendored
specifications under `interop/specs/vendor` and from the cluster's own
generated identities. What is left is named one deployment variable at a time
and refused by name when it is absent, so the bring-up needs no hand-authored
document. `LAYERX_BETA_INTEROP_MANIFEST_FILE` stays available as an optional
override that wins field by field.

No conformance suite is synthesised. Every adapter and every transport binding
this gateway declares carries its own vectors in this repository, so all eight
conformance pins are derived and no `LAYERX_BETA_INTEROP_CONFORMANCE_*` variable
is required: `interop/specs/conformance/<adapter>` and
`interop/specs/conformance/transport-<binding>` are the suites, and the
identifier, the vector count and the SHA-256 are derived from the same files the
adapter's tests read, so the pinned suite is the exercised suite and the variable
only overrides it. An adapter or binding that carried no vectors here would stay
a deployment input, refused by name until it is declared, and
`interop/specs/vendor/CONFORMANCE.md` records which upstreams publish one.

The trust roots whose counterparty is one of the testnet's own clients are the
beta roots the bring-up generates and passes with `--beta-roots-file`; the
variables override them with a real external counterparty.
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

CONFORMANCE_DIRECTORY = "interop/specs/conformance"
SUITE_NAME = "layerx-%s-conformance-v1"
TRANSPORT_CONFORMANCE = {identifier: "transport-%s" % identifier for identifier in TRANSPORTS}
IN_CLUSTER_SOURCE = "in-cluster default"
BETA_SOURCE = "testnet-generated beta trust root"
BETA_AP2_USE_CASES = ("checkout-mandate", "payment-mandate", "merchant-checkout")
BETA_MERCHANT_ID = "layerx-beta-merchant"
BETA_MERCHANT_NAME = "LayerX Beta Testnet Merchant"
BETA_MERCHANT_PATH = "/checkout"
BETA_VISA_KEY_ID = "layerx-beta-tap-key-1"
BETA_VISA_AGENT_ID = "layerx-beta-trusted-agent"
BETA_FIAT_PROVIDER = "layerx-beta-fiat-provider"

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
SEC1_PATTERN = re.compile(r"^04[0-9a-f]{128}$")
AUDIENCE_PATTERN = re.compile(r"^https://[a-z0-9][a-z0-9.-]*(:[1-9][0-9]{0,4})?$")
CURRENCY_PATTERN = re.compile(r"^[A-Z]{3}$")


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


def first_party_suite(root, identifier):
    """The adapter's own vectors in this repository, or None when it has none.

    The count and the digest come from the very files the adapter's tests read,
    so the configuration pins the suite that is actually exercised. A file
    holding an array contributes one vector per record; any other file is one
    vector.
    """
    directory = root / CONFORMANCE_DIRECTORY / identifier
    if not directory.is_dir():
        return None
    files = sorted(
        (path for path in directory.rglob("*.json") if path.is_file()),
        key=lambda path: path.relative_to(directory).as_posix(),
    )
    if not files:
        return None
    digest = hashlib.sha256()
    count = 0
    for path in files:
        data = path.read_bytes()
        digest.update(path.relative_to(directory).as_posix().encode("utf-8"))
        digest.update(b"\0")
        digest.update(data)
        digest.update(b"\0")
        try:
            records = json.loads(data)
        except ValueError as error:
            raise Refused(
                ["%s does not hold a conformance vector: %s" % (path.relative_to(root), error)]
            )
        if isinstance(records, list):
            if not records:
                raise Refused(["%s carries no vector" % path.relative_to(root)])
            count += len(records)
        else:
            count += 1
    return SUITE_NAME % identifier, count, digest.hexdigest()


def derived(root):
    """Every configuration field this repository can compute for itself."""
    vendor = root / "interop/specs/vendor"
    adapters = {}
    for identifier in ADAPTERS:
        if identifier in VENDORED_DOCUMENT:
            document = vendor / VENDORED_DOCUMENT[identifier]
        else:
            document = root / FIAT_DOCUMENT
        suite = first_party_suite(root, identifier)
        if suite is None:
            conformance = {
                field: (None, CONFORMANCE_VARIABLE[identifier])
                for field in ("conformance_suite", "conformance_vectors", "conformance_sha256")
            }
        else:
            name, count, suite_sha256 = suite
            source = "%s/%s" % (CONFORMANCE_DIRECTORY, identifier)
            conformance = {
                "conformance_suite": (name, source),
                "conformance_vectors": (count, source),
                "conformance_sha256": (suite_sha256, source),
            }
        adapters[identifier] = {
            "specification": (SPECIFICATION[identifier], "vendored specification"),
            "version": (VERSION[identifier], "vendored specification"),
            "specification_sha256": (document_digest(document), str(document.relative_to(root))),
            "evidence_policy": (EVIDENCE[identifier], "adapter evidence policy"),
            **conformance,
        }
    transports = {}
    for identifier in TRANSPORTS:
        document = vendor / TRANSPORT_DOCUMENT[identifier]
        directory = TRANSPORT_CONFORMANCE[identifier]
        suite = first_party_suite(root, directory)
        if suite is None:
            conformance = (None, TRANSPORT_VARIABLE[identifier])
        else:
            conformance = (suite[2], "%s/%s" % (CONFORMANCE_DIRECTORY, directory))
        transports[identifier] = {
            "version": (TRANSPORT_VERSION, "vendored transport binding"),
            "specification_sha256": (document_digest(document), str(document.relative_to(root))),
            "conformance_sha256": conformance,
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


def beta_roots(path, reasons):
    """The trust roots the bring-up generates for the testnet's own clients.

    The bring-up generates the key material and writes the cluster facts that
    go with it; the shape of each root is built here so it is exercised by
    `--self-test`. Every identifier says the root is testnet-generated.
    """
    try:
        material = json.loads(pathlib.Path(path).read_text())
    except (OSError, ValueError) as error:
        reasons.append("the generated beta trust roots %s are unreadable: %s" % (path, error))
        return None
    if not isinstance(material, dict):
        reasons.append("the generated beta trust roots %s must hold a JSON object" % path)
        return None
    problems = []

    def hex32(name):
        value = material.get(name)
        if not isinstance(value, str) or not DIGEST_PATTERN.match(value):
            problems.append("%s must be a 32-byte lowercase hexadecimal value" % name)
            return None
        return value

    keys = material.get("ap2_keys")
    if not isinstance(keys, dict) or sorted(keys) != sorted(BETA_AP2_USE_CASES):
        problems.append("ap2_keys must hold one key per AP2 use case: %s" % ", ".join(BETA_AP2_USE_CASES))
        keys = {}
    for use_case, key in sorted(keys.items()):
        if not isinstance(key, str) or not SEC1_PATTERN.match(key):
            problems.append("the %s key must be an uncompressed SEC1 P-256 public key" % use_case)
    audience = material.get("audience")
    if not isinstance(audience, str) or not AUDIENCE_PATTERN.match(audience):
        problems.append("audience must be the https origin the mandates are issued for")
        audience = None
    currency = material.get("currency")
    if not isinstance(currency, str) or not CURRENCY_PATTERN.match(currency):
        problems.append("currency must be the three-letter code of the cluster asset")
    decimals = material.get("asset_decimals")
    if not isinstance(decimals, int) or isinstance(decimals, bool) or not 0 <= decimals <= 38:
        problems.append("asset_decimals must be the decimal exponent of the cluster asset")
        decimals = None
    expires_at = material.get("visa_agent_expires_at")
    if not isinstance(expires_at, int) or isinstance(expires_at, bool) or expires_at <= 0:
        problems.append("visa_agent_expires_at must be the expiry of the generated agent key")
    agent_key = material.get("visa_agent_public_key")
    if not isinstance(agent_key, str) or not DIGEST_PATTERN.match(agent_key):
        problems.append("visa_agent_public_key must be a 32-byte ed25519 public key")
    values = {name: hex32(name) for name in (
        "principal_digest",
        "layerx_agent",
        "payer_account",
        "payee_account",
        "asset",
        "fiat_provider_public_key",
    )}
    if problems:
        reasons.extend(
            "the generated beta trust roots %s are incomplete: %s" % (path, problem)
            for problem in problems
        )
        return None
    authority = audience[len("https://"):]
    return {
        "ap2_keys": [
            {
                "use_case": use_case,
                "key_id": "layerx-beta-%s-key" % use_case,
                "public_key_sec1": keys[use_case],
            }
            for use_case in BETA_AP2_USE_CASES
        ],
        "ap2_assets": [
            {
                "principal_digest": values["principal_digest"],
                "audience": audience,
                "currency": currency,
                "minor_unit_exponent": 0,
                "atomic_units_per_minor_unit": str(10 ** decimals),
                "asset": values["asset"],
                "payer_account": values["payer_account"],
                "payee_account": values["payee_account"],
                "payee_merchant_id": BETA_MERCHANT_ID,
                "payee_merchant_name": BETA_MERCHANT_NAME,
            }
        ],
        "visa_agents": [
            {
                "key_id": BETA_VISA_KEY_ID,
                "agent_id": BETA_VISA_AGENT_ID,
                "agent_domain": audience,
                "layerx_agent": values["layerx_agent"],
                "algorithm": "ed25519",
                "public_key": agent_key,
                "status": "active",
                "expires_at": expires_at,
            }
        ],
        "visa_targets": [
            {
                "principal_digest": values["principal_digest"],
                "authority": authority,
                "path": BETA_MERCHANT_PATH,
            }
        ],
        "fiat_providers": [
            {
                "provider": BETA_FIAT_PROVIDER,
                "public_key_ed25519": values["fiat_provider_public_key"],
            }
        ],
    }


def cluster_roots(environ, network_id, sequencer_public_key, beta, reasons):
    """The trust roots, defaulting to the cluster's own generated identities."""
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
            if beta is not None:
                roots[root] = (beta[root], BETA_SOURCE)
            elif network_id is None:
                roots[root] = (None, BETA_SOURCE)
            continue
        if network_id is None or sequencer_public_key is None:
            roots[root] = (None, IN_CLUSTER_SOURCE)
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
                IN_CLUSTER_SOURCE,
            )
        else:
            roots[root] = (
                {
                    "id": "layerx-ucp-handler",
                    "version": UCP_REVISION,
                    "spec": "https://ucp.dev/%s/specification/checkout/" % UCP_REVISION,
                    "schema": "https://ucp.dev/%s/schemas/shopping/checkout.json" % UCP_REVISION,
                },
                IN_CLUSTER_SOURCE,
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


def validated(adapters, transports, roots, check_only, reasons, sources=None):
    document = {"adapters": [], "transports": []}
    for identifier in ADAPTERS:
        entry = adapters[identifier]
        suite, suite_source = entry["conformance_suite"]
        count, count_source = entry["conformance_vectors"]
        digest, digest_source = entry["conformance_sha256"]
        if suite is None or count is None or digest is None:
            reasons.append(
                "%s is required: the imported %s conformance suite as '%s'; this repository holds "
                "no %s vectors under %s and no upstream suite is vendorable "
                "(interop/specs/vendor/CONFORMANCE.md)"
                % (
                    CONFORMANCE_VARIABLE[identifier],
                    identifier,
                    CONFORMANCE_FORM,
                    identifier,
                    CONFORMANCE_DIRECTORY,
                )
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
        if value is None and check_only and source in (IN_CLUSTER_SOURCE, BETA_SOURCE):
            continue
        if value is None:
            reasons.append(
                "%s is required: %s is a counterparty credential this cluster does not hold and "
                "no generated beta root was supplied with --beta-roots-file"
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
        if sources is not None:
            sources[root] = source
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


def render(
    root,
    environ,
    network_id=None,
    sequencer_public_key=None,
    beta_roots_file=None,
    sources=None,
):
    """Return the gateway configuration document or raise `Refused`."""
    reasons = []
    adapters, transports = derived(root)
    conformance_variables(environ, adapters, transports, reasons)
    beta = beta_roots(beta_roots_file, reasons) if beta_roots_file else None
    roots = cluster_roots(environ, network_id, sequencer_public_key, beta, reasons)
    manifest = environ.get(OVERRIDE_VARIABLE, "").strip()
    if manifest:
        override(manifest, adapters, transports, roots, reasons)
    document = validated(adapters, transports, roots, network_id is None, reasons, sources)
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
    parser.add_argument("--beta-roots-file")
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
        sources = {}
        document = render(
            root,
            os.environ,
            arguments.network_id,
            sequencer_key(arguments.sequencer_public_key_file),
            arguments.beta_roots_file,
            sources,
        )
    except Refused as refusal:
        sys.stderr.write("the interop gateway configuration was refused:\n")
        for reason in refusal.reasons:
            sys.stderr.write("  - %s\n" % reason)
        return 2
    out = pathlib.Path(arguments.out)
    out.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n")
    out.chmod(0o600)
    generated = sorted(name for name, source in sources.items() if source == BETA_SOURCE)
    if generated:
        sys.stdout.write(
            "interop gateway trust roots generated for this testnet and named layerx-beta-*: %s\n"
            % ", ".join(generated)
        )
    return 0


def self_test():
    """Exercise the real render against the documents and vectors in this checkout."""
    import shutil
    import tempfile

    root = repository_root()
    vendor = root / "interop/specs/vendor"
    network, key = "layerx-testnet", "ab" * 32
    first_party = {
        identifier: first_party_suite(root, identifier)
        for identifier in ADAPTERS
        if first_party_suite(root, identifier) is not None
    }
    assert sorted(first_party) == sorted(ADAPTERS), sorted(first_party)
    transport_first_party = {
        identifier: first_party_suite(root, TRANSPORT_CONFORMANCE[identifier])
        for identifier in TRANSPORTS
        if first_party_suite(root, TRANSPORT_CONFORMANCE[identifier]) is not None
    }
    assert sorted(transport_first_party) == sorted(TRANSPORTS), sorted(transport_first_party)
    suites = {
        identifier: "%s-owner-vectors,%d,%s" % (identifier.replace("-", "_"), 8, "33" * 32)
        for identifier in ADAPTERS
        if identifier not in first_party
    }
    complete = {CONFORMANCE_VARIABLE[identifier]: value for identifier, value in suites.items()}
    complete.update(
        {
            TRANSPORT_VARIABLE[identifier]: "66" * 32
            for identifier in TRANSPORTS
            if identifier not in transport_first_party
        }
    )
    complete.update(
        {
            ROOT_VARIABLE["ap2_keys"]: json.dumps(
                [
                    {
                        "use_case": "checkout-mandate",
                        "key_id": "k1",
                        "public_key_sec1": "04" + "ab" * 64,
                    }
                ]
            ),
            ROOT_VARIABLE["ap2_assets"]: json.dumps([{"principal_digest": "33" * 32}]),
            ROOT_VARIABLE["visa_agents"]: json.dumps([{"key_id": "tap-1"}]),
            ROOT_VARIABLE["visa_targets"]: json.dumps([{"authority": "shop.example"}]),
            ROOT_VARIABLE["fiat_providers"]: json.dumps(
                [{"provider": "example-provider", "public_key_ed25519": "dd" * 32}]
            ),
        }
    )

    def refusal(environ, network_id=network, public_key=key, beta_file=None):
        try:
            render(root, environ, network_id, public_key, beta_file)
        except Refused as refused:
            return refused.reasons
        raise AssertionError("the render accepted %r" % sorted(environ))

    reasons = refusal({})
    for identifier in ADAPTERS:
        variable = CONFORMANCE_VARIABLE[identifier]
        named = any(reason.startswith(variable) for reason in reasons)
        assert named == (identifier not in first_party), identifier
    for identifier in TRANSPORTS:
        variable = TRANSPORT_VARIABLE[identifier]
        named = any(reason.startswith(variable) for reason in reasons)
        assert named == (identifier not in transport_first_party), identifier
    assert not any(
        reason.startswith("LAYERX_BETA_INTEROP_CONFORMANCE_") for reason in reasons
    ), reasons
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
    for identifier in ADAPTERS:
        assert adapters[identifier]["evidence_policy"] == EVIDENCE[identifier]

    transport_tests = (root / "interop/crates/layerx-x402/tests/transports.rs").read_text()
    tests = {
        "x402": (root / "interop/crates/layerx-x402/tests/vectors.rs").read_text(),
        "ap2": (root / "interop/crates/layerx-ap2/tests/mandates.rs").read_text(),
        "ucp": (root / "interop/crates/layerx-ucp/tests/conformance_vectors.rs").read_text(),
        "visa-tap": (root / "interop/crates/layerx-visa-tap/tests/conformance.rs").read_text(),
        "fiat": (root / "interop/crates/layerx-fiat/tests/adapter.rs").read_text(),
    }
    tests.update(
        {TRANSPORT_CONFORMANCE[identifier]: transport_tests for identifier in TRANSPORTS}
    )

    def exercised(directory_name, declared, source_text):
        """The vector files are the suite: every record counted, read and pinned."""
        name, count, digest = declared
        directory = root / CONFORMANCE_DIRECTORY / directory_name
        assert name == SUITE_NAME % directory_name and SUITE_PATTERN.match(name), name
        vector_records = 0
        for vector_file in sorted(directory.rglob("*.json")):
            records = json.loads(vector_file.read_text())
            vector_records += len(records) if isinstance(records, list) else 1
            included = 'include_str!("../../../specs/conformance/%s/%s")' % (
                directory_name,
                vector_file.relative_to(directory).as_posix(),
            )
            assert included in source_text, included
        assert count == vector_records and count > 0, directory_name
        with tempfile.TemporaryDirectory() as directory_copy:
            copy_root = pathlib.Path(directory_copy)
            copied = copy_root / CONFORMANCE_DIRECTORY / directory_name
            shutil.copytree(directory, copied)
            assert first_party_suite(copy_root, directory_name) == (name, count, digest)
            edited = sorted(copied.rglob("*.json"))[0]
            edited.write_bytes(edited.read_bytes() + b" ")
            assert first_party_suite(copy_root, directory_name)[2] != digest

    for identifier, declared in first_party.items():
        name, count, digest = declared
        assert adapters[identifier]["conformance_suite"] == name
        assert adapters[identifier]["conformance_vectors"] == count
        assert adapters[identifier]["conformance_sha256"] == digest
        exercised(identifier, declared, tests[identifier])
    assert adapters["x402"]["conformance_vectors"] == 18
    assert adapters["ap2"]["conformance_vectors"] == 6
    assert adapters["ucp"]["conformance_vectors"] == 26
    assert adapters["visa-tap"]["conformance_vectors"] == 23
    assert adapters["fiat"]["conformance_vectors"] == 26

    overriding = dict(complete)
    overriding[CONFORMANCE_VARIABLE["x402"]] = "owner-x402-suite,4,%s" % ("77" * 32)
    overridden = {
        entry["id"]: entry
        for entry in render(root, overriding, network, key)["adapters"]
    }
    assert overridden["x402"]["conformance_suite"] == "owner-x402-suite"
    assert overridden["x402"]["conformance_vectors"] == 4
    assert overridden["x402"]["conformance_sha256"] == "77" * 32
    assert overridden["ap2"]["conformance_sha256"] == first_party["ap2"][2]

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
        declared = transport_first_party[identifier]
        assert entry["conformance_sha256"] == declared[2]
        exercised(
            TRANSPORT_CONFORMANCE[identifier],
            declared,
            tests[TRANSPORT_CONFORMANCE[identifier]],
        )
    assert all(declared[1] == 8 for declared in transport_first_party.values())
    transport_override = dict(complete)
    transport_override[TRANSPORT_VARIABLE["a2a"]] = "99" * 32
    overridden_transports = {
        entry["id"]: entry
        for entry in render(root, transport_override, network, key)["transports"]
    }
    assert overridden_transports["a2a"]["conformance_sha256"] == "99" * 32
    assert overridden_transports["http"]["conformance_sha256"] == transport_first_party["http"][2]
    for broken, expected in (
        ("00" * 32, "zero digest is not a pin"),
        ("nothex", "hexadecimal digest"),
    ):
        environ = dict(complete)
        environ[TRANSPORT_VARIABLE["mcp"]] = broken
        reasons = refusal(environ)
        assert any(expected in reason for reason in reasons), (broken, reasons)
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

    material = {
        "ap2_keys": {use_case: "04" + "ab" * 64 for use_case in BETA_AP2_USE_CASES},
        "visa_agent_public_key": "cc" * 32,
        "visa_agent_expires_at": 1893456000,
        "fiat_provider_public_key": "dd" * 32,
        "principal_digest": "11" * 32,
        "layerx_agent": "22" * 32,
        "payer_account": "22" * 32,
        "payee_account": "44" * 32,
        "asset": "55" * 32,
        "audience": "https://layerx-interop-gateway.layerx-testnet.svc.cluster.local:9443",
        "currency": "LXT",
        "asset_decimals": 18,
    }
    with tempfile.TemporaryDirectory() as directory:
        roots_file = pathlib.Path(directory) / "interop-beta-roots.json"
        roots_file.write_text(json.dumps(material))
        environ = {
            variable: value
            for variable, value in complete.items()
            if variable not in {ROOT_VARIABLE[name] for name in EXTERNAL_ROOTS}
        }
        sources = {}
        generated = render(root, dict(environ), network, key, str(roots_file), sources)
        for name in EXTERNAL_ROOTS:
            assert sources[name] == BETA_SOURCE, name
        assert [pin["use_case"] for pin in generated["ap2_keys"]] == list(BETA_AP2_USE_CASES)
        assert all(
            pin["key_id"].startswith("layerx-beta-")
            and pin["public_key_sec1"] == material["ap2_keys"][pin["use_case"]]
            for pin in generated["ap2_keys"]
        )
        binding = generated["ap2_assets"][0]
        assert binding["principal_digest"] == material["principal_digest"]
        assert binding["audience"] == material["audience"] and binding["currency"] == "LXT"
        assert binding["minor_unit_exponent"] == 0
        assert binding["atomic_units_per_minor_unit"] == "1" + "0" * 18
        assert int(binding["atomic_units_per_minor_unit"]) < 2**128
        assert binding["asset"] == material["asset"]
        assert binding["payer_account"] == material["payer_account"]
        assert binding["payee_account"] == material["payee_account"]
        assert binding["payee_merchant_id"] == BETA_MERCHANT_ID
        agent = generated["visa_agents"][0]
        assert agent["algorithm"] == "ed25519" and agent["status"] == "active"
        assert agent["public_key"] == material["visa_agent_public_key"]
        assert agent["layerx_agent"] != agent["public_key"]
        assert agent["agent_domain"].startswith("https://") and agent["expires_at"] > 0
        target = generated["visa_targets"][0]
        assert target["authority"] == material["audience"][len("https://") :]
        assert target["path"] == BETA_MERCHANT_PATH and "/" not in target["authority"]
        assert generated["fiat_providers"] == [
            {
                "provider": BETA_FIAT_PROVIDER,
                "public_key_ed25519": material["fiat_provider_public_key"],
            }
        ]
        declared = dict(environ)
        declared[ROOT_VARIABLE["fiat_providers"]] = complete[ROOT_VARIABLE["fiat_providers"]]
        pinned = render(root, declared, network, key, str(roots_file))
        assert pinned["fiat_providers"][0]["provider"] == "example-provider"
        assert pinned["visa_agents"] == generated["visa_agents"]

        for field, value, expected in (
            ("ap2_keys", {"checkout-mandate": "04" + "ab" * 64}, "one key per AP2 use case"),
            ("audience", "http://gateway.internal", "https origin"),
            ("currency", "lxt", "three-letter code"),
            ("asset_decimals", 39, "decimal exponent"),
            ("visa_agent_expires_at", 0, "expiry of the generated agent key"),
            ("visa_agent_public_key", "cc" * 31, "ed25519 public key"),
            ("principal_digest", "not-hex", "32-byte lowercase hexadecimal"),
        ):
            broken_material = dict(material)
            broken_material[field] = value
            roots_file.write_text(json.dumps(broken_material))
            reasons = refusal(dict(environ), beta_file=str(roots_file))
            assert any(expected in reason for reason in reasons), (field, reasons)
        broken_material = dict(material)
        broken_material["ap2_keys"] = {
            use_case: "ab" * 65 for use_case in BETA_AP2_USE_CASES
        }
        roots_file.write_text(json.dumps(broken_material))
        assert any(
            "uncompressed SEC1 P-256 public key" in reason
            for reason in refusal(dict(environ), beta_file=str(roots_file))
        )
        roots_file.write_text("{")
        assert any(
            "are unreadable" in reason
            for reason in refusal(dict(environ), beta_file=str(roots_file))
        )

    with tempfile.TemporaryDirectory() as directory:
        manifest = pathlib.Path(directory) / "manifest.json"
        manifest.write_text(
            json.dumps(
                {
                    "adapters": {
                        "ucp": {"conformance_suite": "owner-ucp-suite", "version": "20260409"}
                    },
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
        assert transports["http"]["conformance_sha256"] == transport_first_party["http"][2]
        assert overridden["ucp_payment_handler"]["id"] == "owner-handler"
        assert overridden["x402_supported"] == document["x402_supported"]

        manifest.write_text(json.dumps({"adapters": {"ucp": {"conformance_vectors": 0}}}))
        assert any("not a conformance suite" in reason for reason in refusal(environ))
        manifest.write_text(json.dumps({"adapters": {"ucp": {"unknown": 1}}}))
        assert any("is not a configured field" in reason for reason in refusal(environ))
        manifest.write_text(json.dumps({"unexpected": 1}))
        assert any("unknown fields" in reason for reason in refusal(environ))

    required = len(suites) + sum(
        1 for identifier in TRANSPORTS if identifier not in transport_first_party
    )
    derived_suites = len(first_party) + len(transport_first_party)
    sys.stdout.write(
        "interop gateway render: %d adapters and %d transports derived from the vendored "
        "specifications; %d first-party conformance suites derived from %s; %d conformance "
        "variables refused by name when absent, %d optional conformance overrides and %d "
        "optional trust-root overrides\n"
        % (
            len(ADAPTERS),
            len(TRANSPORTS),
            derived_suites,
            CONFORMANCE_DIRECTORY,
            required,
            derived_suites,
            len(ROOTS) + 1,
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
