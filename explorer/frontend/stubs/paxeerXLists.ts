import type { PaxeerXAnchorsItem, PaxeerXReceiptsItem, PaxeerXTxStatus } from 'types/api/paxeerXLists';

import { TX_HASH } from './tx';

export const PAXEER_X_ANCHORS_ITEM: PaxeerXAnchorsItem = {
  batch_number: 1042,
  checkpoint_id: TX_HASH,
  checkpoint_height: 8340992,
  sealed_height: 8340960,
  state_root: TX_HASH,
  block_number: 8341004,
  timestamp: '2023-05-22T18:00:36.000000Z',
};

export const PAXEER_X_RECEIPTS_ITEM: PaxeerXReceiptsItem = {
  id: TX_HASH,
  account: '0000000000000000000000000000000000000000000000000000000000000000',
  status: 'sealed',
  block_number: 8341004,
};

export const PAXEER_X_TX_STATUS: PaxeerXTxStatus = {
  rung: 'sealed',
  block_number: 8341004,
  sealed_batch_number: 1042,
  finalized_batch_number: 1041,
  checkpoint_id: TX_HASH,
};
