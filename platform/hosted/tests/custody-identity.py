import json
from pathlib import Path
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "tests/bridge"))
from deploy_local_custody import PERSISTENT_GENESIS, disposable_rpc


def verify(path):
    identity = json.loads(path.read_text())
    ca = identity["ca_bundle"]
    for origin in identity["rpc_origins"]:
        rpc = disposable_rpc(origin, ca, path)
        assert rpc.disposable and rpc.comet_chain_id == identity["comet_chain_id"]
        assert "0x" + rpc.genesis_sha256.hex() == identity["genesis_sha256"]
    with tempfile.TemporaryDirectory(dir=path.parent) as directory:
        altered = Path(directory) / "identity.json"
        for field, value, message in (
                ("genesis_sha256", "0x" + PERSISTENT_GENESIS.hex(), "persistent chain genesis refused"),
                ("comet_chain_id", identity["comet_chain_id"] + "-different", "disposable genesis identity"),
                ("genesis_sha256", "0x" + "00" * 32, "persistent chain genesis refused"),
                ("chain_id", 126, "disposable chain ID"),
                ("ca_sha256", "0x" + "00" * 32, "disposable CA pin")):
            altered.write_text(json.dumps({**identity, field: value}))
            try:
                disposable_rpc(identity["rpc_origins"][0], ca, altered)
            except ValueError as error:
                assert str(error) == message, str(error)
            else:
                raise AssertionError("accepted " + field)
            print("Refused " + field + ": " + message)
    print("Both real disposable origins accepted with exact genesis, Comet and EVM pins")


if __name__ == "__main__":
    verify(Path(sys.argv[1]))
