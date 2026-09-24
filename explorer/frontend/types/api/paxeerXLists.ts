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
