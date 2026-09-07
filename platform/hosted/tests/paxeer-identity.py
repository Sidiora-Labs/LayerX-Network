import hashlib
import json
from pathlib import Path
import ssl
import sys
import urllib.request


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise ValueError("genesis redirect refused")


def verify(path, comet_chain_id):
    identity = json.loads(path.read_text())
    origins = identity["rpc_origins"]
    assert len(origins) == 2 and len(set(origins)) == 2
    assert comet_chain_id
    ca = Path(identity["ca_bundle"])
    opener = urllib.request.build_opener(
        NoRedirect(), urllib.request.HTTPSHandler(context=ssl.create_default_context(cafile=ca)))
    bodies = []
    for origin in origins:
        assert origin.startswith("https://")
        with opener.open(origin + "/genesis", timeout=30) as response:
            body = response.read(32 * 1024 * 1024 + 1)
            assert len(body) <= 32 * 1024 * 1024
            assert response.headers.get_all("X-LayerX-Genesis-SHA256") == [hashlib.sha256(body).hexdigest()]
        assert json.loads(body)["chain_id"] == comet_chain_id
        bodies.append(body)
        request = urllib.request.Request(origin, data=json.dumps({
            "jsonrpc": "2.0", "id": 1, "method": "eth_chainId", "params": []}).encode(),
            headers={"Content-Type": "application/json"})
        with opener.open(request, timeout=20) as response:
            raw = response.read(65537)
        assert len(raw) <= 65536
        result = json.loads(raw)
        assert result["jsonrpc"] == "2.0" and result["id"] == 1 and "error" not in result
        assert int(result["result"], 16) == 125
    assert bodies[0] == bodies[1]
    identity.update(genesis_source="boundary", genesis_sha256="0x" + hashlib.sha256(bodies[0]).hexdigest(),
                    comet_chain_id=comet_chain_id, chain_id=125,
                    ca_sha256="0x" + hashlib.sha256(ca.read_bytes()).hexdigest())
    path.write_text(json.dumps(identity, indent=2) + "\n")
    print(json.dumps({key: identity[key] for key in ("genesis_sha256", "comet_chain_id", "chain_id")}))


if __name__ == "__main__":
    verify(Path(sys.argv[1]), sys.argv[2])
