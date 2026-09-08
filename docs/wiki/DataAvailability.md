# Data availability

The canonical availability commitment is `lxp_batch_availability_root(body, arena, root)`. It builds the five-class DA bundle with `LXP_DA_CANONICAL_CHUNK_BYTES` (65,536 bytes) and commits the ordered, metadata-bound chunk hashes. Activity, receipt, event and oracle roots remain independent commitments over their records.

Activities and oracle inputs use the native sequence encoding: a big-endian record count followed by length-prefixed records. The receipt class contains tagged length-prefixed records, all kind-1 receipts followed by kind-2 events. The state diff is an account-ID-sorted sequence of changed account IDs and canonical post-batch leaf values. Replay binds a real kernel, snapshots its account registry before transitions and compares the recomputed diff after replay. Recovery metadata contains the settled kernel's module roots, canonical account frontier and sequence/watermark values.

Prepared publication carries state diff and recovery metadata in WAL version 4. The existing WAL byte bound remains enforced. Canonical bodies have a separate `da-bodies.log` under `LAYERX_NODE_CHECKPOINT_DIRECTORY`; the batch log remains header-only. Served bundles are stored in the sibling `da` directory before publication completes. Body-log records supply independent commitment reconstruction when a served bundle is unavailable.

`LAYERX_NODE_DA_RETAIN_BATCHES` defaults to 100000 and rejects values below 1024. Retention preserves the newest finalized checkpoint; without a finalized checkpoint, pruning retains all bundles. Startup checks the retained interval against signed retained headers before advertising `availability_fetch`. A missing or corrupt served bundle withholds that capability. Fetch independently verifies the stored bundle again.

The tag-18 request has empty proof material and one canonical selector:

- `01 || checkpoint_id[32]`: the batch covered by that finalized certificate.
- `02 || batch:u64be`: one retained batch.
- `03 || first:u64be || last:u64be`: the inclusive sequence range.
- `04 || activity_id[32]`: the activity's batch.
- `05 || batch:u64be`: one durable, sealed, header-signed candidate regardless of finalization.

Resolution permits at most eight batches. Because the shipped client consumes one batch per request, every multi-batch result is refused. A sequence selector must lie entirely inside its resolved batch. Selectors 01–04 refuse batches newer than the latest finalized batch. Selector 05 permits those batches and works with no registered checkpoint. It uses exactly the same authenticated UID/GID principal set as tag 28; other principals receive the existing unauthorized refusal. Unknown, unsealed, incomplete or corrupt candidates fail closed. Candidate fetch never marks a batch finalized.

Each tag-19 response carries exact chunk bytes as its canonical payload. Proof material is `batch:u64be || index:u32be || class:u8 || class_offset:u64be || chunk_hash[32] || leaf_index:u32be || leaf_count:u32be || depth:u8 || siblings[depth][32]`. Tag 20 ends the stream with empty payload and proof material.

Malformed selectors return `LXP_ERR_MALFORMED_ENVELOPE`; reversed or zero-start ranges return `LXP_ERR_NON_CANONICAL`; unknown selections return `LXP_ERR_UNKNOWN_ACTIVITY`; multi-batch or over-limit resolutions return `LXP_ERR_LENGTH_LIMIT`; ranges extending beyond their resolved batch return `LXP_ERR_BATCH_GAP`; unavailable capability or unfinalized candidates selected by 01–04 return `LXP_ERR_DA_MISSING`. Storage and proof failures retain their typed refusal results.

Checkpoint candidates use the existing tag-12 signed batch header. No tag 32 is introduced. Validity proof bytes remain opaque to verifiers; beta certificates use an empty validity proof.

This implementation remains unqualified. Real-kernel replay fixture migration, daemon recovery and retention tests, and real-daemon Rust selector/corruption tests must complete before the required build, sanitizer, daemon and Rust gates can establish runtime evidence.

`layerx_client::availability::fetch_sealed_candidate` uses the same chunk, five-class completeness, ordering, bounds and record-root verification as finalized retrieval. Supply commitments from the verified signed tag-12 header; the canonical chunk size remains 65,536 bytes. A guarantor must verify and replay the candidate before attesting. Retrieval alone is not replay or finality evidence.
