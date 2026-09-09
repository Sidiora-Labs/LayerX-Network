# LayerX explorer index

Rebuildable public projections over verified receipt, checkpoint, account and program evidence. The index does not supply transaction authority.

Program ingestion combines verified registry records with current protocol balance proofs. It refuses inconsistent bindings, stale balances, conflicting evidence and head regression. Mirror receipt and state reads retain the verified signed-header digest and source provenance.

The program projection borrows verified balances; it preserves the proof checks while avoiding an unnecessary clone. Public fallible interfaces document their refusal conditions.

Qualification commands from the repository root:

```sh
cargo clippy --locked --manifest-path human/Cargo.toml -p layerx-explorer-index --all-targets -- -D warnings
make BUILD_DIR=qual-logs/explorer-native qual-logs/explorer-native/tests/explorer_fixture
LAYERX_EXPLORER_CORE_FIXTURE="$PWD/qual-logs/explorer-native/tests/explorer_fixture" \
  cargo test --locked --manifest-path human/Cargo.toml -p layerx-explorer-index
```
