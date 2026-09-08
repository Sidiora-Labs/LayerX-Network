# layerx-paxeer-client

Typed Paxeer JSON-RPC custody, finality, withdrawal, and emergency-exit boundaries. Production finality requires agreement from at least two independent HTTPS endpoints. Local disposable-chain configurations are explicit.

Deposit proofs are verified against finalized custody receipts, canonical Merkle inclusion, and the configured checkpoint authority. Withdrawal claims bind a verified LayerX debit receipt to checkpoint membership and the certificate recorded by the deployed checkpoint registry. Codec admission checks canonical structure; settlement authorization occurs at `WithdrawalBoundary` against current contract state.

The real contract withdrawal tests exercise protocols 2 and 3 and reject a checkpoint certificate whose signature is changed after registration. They deploy actual contracts on a disposable Anvil process and require the Foundry `anvil`, `forge`, and `cast` tooling. The workspace tests also cover finality, deposits, exits, and custody evidence.
