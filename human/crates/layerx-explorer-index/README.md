# LayerX explorer index

Rebuildable public projections over verified receipt, checkpoint, account and program evidence. The index does not supply transaction authority.

Program ingestion combines verified registry records with current protocol balance proofs. It refuses inconsistent bindings, stale balances, conflicting evidence and head regression. Mirror receipt and state reads retain the verified signed-header digest and source provenance.

The program projection borrows verified balances; it preserves the proof checks while avoiding an unnecessary clone. Public fallible interfaces document their refusal conditions.

The program service requires `LAYERX_EXPLORER_AUTHORITY_CA_DER` to name a readable DER certificate file, at most 64 KiB. This CA authenticates both HTTPS endpoints configured by `LAYERX_EXPLORER_NODE_ENDPOINT` and `LAYERX_EXPLORER_AUTHORITY_ENDPOINT`. The authority endpoint must identify a separate replica, bound by `LAYERX_EXPLORER_AUTHORITY_REPLICA_ID`; the CA does not replace the signed protocol evidence or sequencer trust history. Missing, empty, oversized or malformed CA input refuses startup.

Qualification commands from the repository root:

```sh
cargo clippy --locked --manifest-path human/Cargo.toml -p layerx-explorer-index --all-targets -- -D warnings
make BUILD_DIR=qual-logs/explorer-native qual-logs/explorer-native/tests/explorer_fixture
LAYERX_EXPLORER_CORE_FIXTURE="$PWD/qual-logs/explorer-native/tests/explorer_fixture" \
  cargo test --locked --manifest-path human/Cargo.toml -p layerx-explorer-index
```
