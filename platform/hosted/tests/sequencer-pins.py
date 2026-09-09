import json
from pathlib import Path
import struct
import sys


def provision(directory, output):
    history = (directory / "trust-history").read_bytes()
    domain = b"LayerX/sequencer-trust-history/v1\0"
    assert history.startswith(domain) and len(history) == len(domain) + 4 + 103
    assert struct.unpack_from(">HH", history, len(domain)) == (1, 0)
    entry = history[len(domain) + 4:]
    version, network, epoch = struct.unpack_from(">HIQ", entry)
    assert version == 3 and network > 0 and epoch == 1
    sequencer_id, public_key = entry[14:46].hex(), entry[46:78].hex()
    first, last, revoked, revoked_at = struct.unpack_from(">QQBQ", entry, 78)
    assert 0 < first <= last and revoked == 0 and revoked_at == 0
    assert public_key == (directory / "sequencer-public-key").read_text()
    pins = dict(sequencer_id=sequencer_id, sequencer_public_key=public_key,
                sequencer_first_batch=str(first), sequencer_last_batch=str(last))
    for key in ("sequencer_id", "sequencer_first_batch", "sequencer_last_batch"):
        (directory / key.replace("_", "-")).write_text(pins[key])
    output.write_text(json.dumps({"layerx": pins}, indent=2) + "\n")


def manifests(directory):
    import yaml

    for filename, name, prefix, mount in (
            ("gateway.yaml", "layerx-gateway", "LAYERX_GATEWAY", "/run/layerx/authority"),
            ("developer.yaml", "layerx-webhooks", "LAYERX_WEBHOOKS", "/run/layerx/keys")):
        path = directory / filename
        documents = list(yaml.safe_load_all(path.read_text()))
        workload = next(d for d in documents if d["kind"] == "Deployment" and d["metadata"]["name"] == name)
        pod = workload["spec"]["template"]["spec"]
        container = next(c for c in pod["containers"] if c["name"] == name.removeprefix("layerx-"))
        for suffix in ("ID", "FIRST_BATCH", "LAST_BATCH"):
            env_name = prefix + "_SEQUENCER_" + suffix + "_FILE"
            value = mount + "/sequencer-" + suffix.lower().replace("_", "-")
            existing = [e for e in container["env"] if e["name"] == env_name]
            if existing:
                assert existing == [{"name": env_name, "value": value}]
            else:
                container["env"].append({"name": env_name, "value": value})
        if name == "layerx-webhooks":
            volume = next(v for v in pod["volumes"] if v["name"] == "runtime")
            secret = next(s["secret"] for s in volume["projected"]["sources"]
                          if s.get("secret", {}).get("name") == "layerx-developer-hosted-runtime")
            for suffix in ("id", "first-batch", "last-batch"):
                item = {"key": "sequencer-" + suffix, "path": "keys/sequencer-" + suffix}
                if item not in secret["items"]:
                    assert not any(i["key"] == item["key"] or i["path"] == item["path"] for i in secret["items"])
                    secret["items"].append(item)
        path.write_text(yaml.safe_dump_all(documents, sort_keys=False))


if __name__ == "__main__":
    if sys.argv[1] == "--manifests":
        manifests(Path(sys.argv[2]))
    else:
        provision(Path(sys.argv[1]), Path(sys.argv[2]))
