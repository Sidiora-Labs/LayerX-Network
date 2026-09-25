export const PAXEER_X_STATUS_RUNGS = [ 'pending', 'instant', 'sealed', 'final' ] as const;

export type PaxeerXStatusRung = typeof PAXEER_X_STATUS_RUNGS[number];

export type PaxeerXTxStatus = {
  rung: PaxeerXStatusRung;
  block_number: number | null;
  sealed_batch_number: number | null;
  finalized_batch_number: number | null;
  checkpoint_id: string | null;
};

// `checkpoint_height` is the kernel height the checkpoint commits to and `sealed_height` the
// height it seals; both, like the state root, are optional columns of the anchor log.
export type PaxeerXAnchorsItem = {
  batch_number: number;
  checkpoint_id: string;
  checkpoint_height: number | null;
  sealed_height: number | null;
  state_root: string | null;
  block_number: number;
  timestamp: string;
};

export type PaxeerXAnchorsResponse = {
  items: Array<PaxeerXAnchorsItem>;
  next_page_params: {
    batch_number: number;
    items_count: number;
  } | null;
};

// `status` is the settlement rung of the block the receipt was last seen in; the kernel's own
// verification lattice is served on the single-receipt path only.
export type PaxeerXReceiptsItem = {
  id: string;
  account: string | null;
  status: PaxeerXStatusRung;
  block_number: number;
};

export type PaxeerXReceiptsResponse = {
  items: Array<PaxeerXReceiptsItem>;
  next_page_params: {
    id: string;
    items_count: number;
  } | null;
};

export const PAXEER_X_VERIFICATION_STATUSES = [
  'unverified',
  'sequencer_signed',
  'batch_included',
  'state_proven',
  'checkpoint_finalised',
  'settlement_anchored',
] as const;

export type PaxeerXVerificationStatus = typeof PAXEER_X_VERIFICATION_STATUSES[number];

// One receipt carries, on top of the list item, the rung it has reached on the kernel's own
// verification lattice and the log that recorded it. `payload_hash` is an optional column of the
// receipt log, while the emitting transaction and the timestamp of its block are not.
export type PaxeerXReceipt = PaxeerXReceiptsItem & {
  verification_status: PaxeerXVerificationStatus;
  payload_hash: string | null;
  transaction_hash: string;
  timestamp: string;
};
