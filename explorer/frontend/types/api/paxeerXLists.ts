export const PAXEER_X_STATUS_RUNGS = [ 'pending', 'instant', 'sealed', 'final' ] as const;

export type PaxeerXStatusRung = typeof PAXEER_X_STATUS_RUNGS[number];

export type PaxeerXTxStatus = {
  rung: PaxeerXStatusRung;
  block_number: number | null;
  sealed_batch_number: number | null;
  finalized_batch_number: number | null;
  checkpoint_id: string | null;
};

export type PaxeerXAnchorsItem = {
  batch_number: number;
  checkpoint_id: string;
  checkpoint_height: number;
  sealed_height: number;
  state_root: string;
  block_number: number;
  timestamp: string;
};

export type PaxeerXAnchorsResponse = {
  items: Array<PaxeerXAnchorsItem>;
  next_page_params: {
    checkpoint_height: number;
    items_count: number;
  } | null;
};

export type PaxeerXReceiptsItem = {
  id: string;
  account: string;
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
