import argparse
import json
import subprocess
from pathlib import Path


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--encoder", type=Path, required=True)
    parser.add_argument("--check", action="store_true")
    arguments = parser.parse_args()
    encoded = subprocess.run(
        [str(arguments.encoder.resolve()), "--dump-native-lifecycle"],
        check=True,
        capture_output=True,
        text=True,
    )
    expected = {
        "native-program-deploy-v3": 1,
        "native-program-upgrade-v3": 2,
        "native-program-wind-down-route-v3": 7,
        "native-program-wind-down-deprecate-v3": 7,
        "native-program-wind-down-tombstone-v3": 7,
        "native-program-wind-down-exit-v3": 7,
    }
    documents = {}
    for line in encoded.stdout.splitlines():
        document = json.loads(line)
        name = document["name"]
        if name not in expected or name in documents:
            raise ValueError("native encoder returned an unknown or duplicate vector")
        if (document["protocol_version"], document["module"], document["ordinal"]) != (
            3, 9, expected[name]
        ):
            raise ValueError("native encoder returned a different operation binding")
        payload = bytes.fromhex(document["payload_hex"])
        if not payload or payload.hex() != document["payload_hex"]:
            raise ValueError("native encoder returned non-canonical hexadecimal")
        documents[name] = json.dumps(document, indent=2) + "\n"
    if set(documents) != set(expected):
        raise ValueError("native encoder omitted a lifecycle vector")
    for name, contents in documents.items():
        destination = Path(__file__).with_name(name + ".json")
        if arguments.check:
            if destination.read_text() != contents:
                raise ValueError(f"native lifecycle fixture drift: {destination}")
        else:
            destination.write_text(contents)


if __name__ == "__main__":
    main()
