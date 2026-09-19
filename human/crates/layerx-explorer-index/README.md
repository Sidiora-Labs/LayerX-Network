# LayerX explorer index

Rebuildable public projections over verified receipt, checkpoint, account and program evidence. The index does not supply transaction authority.

Program ingestion combines verified registry records with current protocol balance proofs. It refuses inconsistent bindings, stale balances, conflicting evidence and head regression. Mirror receipt and state reads retain the verified signed-header digest and source provenance.

The program projection borrows verified balances; it preserves the proof checks while avoiding an unnecessary clone. Public fallible interfaces document their refusal conditions.

The program service requires `LAYERX_EXPLORER_AUTHORITY_CA_DER` to name a readable DER certificate file, at most 64 KiB. This CA authenticates both HTTPS endpoints configured by `LAYERX_EXPLORER_NODE_ENDPOINT` and `LAYERX_EXPLORER_AUTHORITY_ENDPOINT`. The authority endpoint must identify a separate replica, bound by `LAYERX_EXPLORER_AUTHORITY_REPLICA_ID`; the CA does not replace the signed protocol evidence or sequencer trust history. Missing, empty, oversized or malformed CA input refuses startup.

`GET /v1/programs/<id>/reads/resolve?name=<name>` resolves a name through the naming reference program. The service reads its identity's next sequence from `GET /v1/dids/<did>/sequence` on the core boundary, signs one noncommitting `resolve` program read at that sequence as its own chain-registered identity, posts it to `POST /v1/programs/read` on the same boundary, and answers only when the returned receipt, terminal payload, call graph and simulation evidence verify against the trusted sequencer key and bind to that request. A resolved name answers `{"name","did","expiry"}` with a 64-character lowercase hexadecimal `did` and a decimal string `expiry`; a name the program refuses as absent or expired answers `404`; a registered program that does not publish the naming reference interface answers `422 not_naming_program`; an invalid name answers `400 invalid_name`; any other verified refusal or any unverifiable answer answers `503`.

Name reads require `LAYERX_EXPLORER_READ_KEY_FILE` (a file holding the hexadecimal ed25519 seed of the read principal), `LAYERX_EXPLORER_READ_ENDPOINT` (`https://<host>:<port>` of the core boundary), `LAYERX_EXPLORER_READ_CA_DER`, `LAYERX_EXPLORER_READ_SEQUENCER_PUBLIC_KEY_FILE`, `LAYERX_EXPLORER_READ_NETWORK_ID`, `LAYERX_EXPLORER_READ_FEE_LIMIT` and `LAYERX_EXPLORER_NAMING_PROGRAM`. Each missing, unreadable or malformed input refuses startup by name. Under protocol 3 the node admits a program read only for an identity that holds an account in the occupancy asset with a balance of at least the signed fee limit, and only when `LAYERX_EXPLORER_READ_FEE_LIMIT` covers the execution ceiling of the declared read resources (25,117,312 fee units at the reference program prices); otherwise the node returns a sequencer-signed refusal and the resolve route answers `503 name_read_refused`. A read never commits, so the balance is never debited. The sequence document only selects the sequence the read is signed at: a wrong value makes the node refuse the read and can never make an answer verify. Startup and `/healthz` sign one read of a probe name against the naming program and report ready only when the answer carries sequencer-signed evidence for that read, which a node returns only for a registered identity.

Qualification commands from the repository root:

```sh
cargo clippy --locked --manifest-path human/Cargo.toml -p layerx-explorer-index --all-targets -- -D warnings
make BUILD_DIR=qual-logs/explorer-native qual-logs/explorer-native/tests/explorer_fixture
LAYERX_EXPLORER_CORE_FIXTURE="$PWD/qual-logs/explorer-native/tests/explorer_fixture" \
  cargo test --locked --manifest-path human/Cargo.toml -p layerx-explorer-index
```
