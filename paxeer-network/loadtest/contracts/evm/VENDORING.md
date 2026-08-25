# Vendored Solidity dependencies

The load-test contracts build without `forge install` or network access.

| Directory | Upstream | Revision | Source archive SHA-256 |
| --- | --- | --- | --- |
| `lib/openzeppelin-contracts` | `https://github.com/OpenZeppelin/openzeppelin-contracts` | `e4f70216d759d8e6a64144a9e1f7bbeed78e7079` (`v5.3.0`) | `49c41e34c683c6593932aa6c5485e621e61ec55d2b9c7c74b5e1aed99c9715a5` |
| `lib/solmate` | `https://github.com/transmissions11/solmate` | `a9e3ea26a2dc73bfa87f0cb189687d029028e0c5` (`v6`) | `2e423b093d439ad0d93469e5f65de473370681bf983347a9a46ee2cf27a9eaa9` |

Only the upstream Solidity source tree and license are retained. `setup.sh`
verifies deterministic tree digests; it never downloads or installs dependencies.
