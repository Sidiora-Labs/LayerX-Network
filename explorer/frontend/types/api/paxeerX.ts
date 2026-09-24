import type { PaxeerXStatusRung } from './paxeerXLists';

// Paxeer X Network unified surfaces.
// `paxeer_x` is the chain-type label; LayerX names only the kernel domain, so kernel-side
// entities keep their `lx_*` / `kernel` spelling here.

export type PaxeerXIdentityKind = 'evm' | 'pax' | 'did' | 'kernel_account';

export interface PaxeerXIdentities {
  evm: string | null;
  pax: string | null;
  did: string | null;
  kernel_account: string | null;
}

export interface PaxeerXAsset {
  id: string;
  denom: string;
  symbol: string | null;
  decimals: number | null;
}

// One balance per asset: `total` is authoritative for display, `parts` explains where it sits.
export interface PaxeerXBalanceParts {
  chain: string;
  custody: string;
  kernel: string;
}

export interface PaxeerXBalance {
  asset: PaxeerXAsset;
  total: string;
  parts: PaxeerXBalanceParts;
}

// Activity rows sit on the same status ladder as transactions and kernel receipts.
export type PaxeerXStatus = PaxeerXStatusRung;

export type PaxeerXActivitySide = 'chain' | 'kernel';

export interface PaxeerXActivityItem {
  kind: string;
  hash: string;
  block_number: number;
  status: PaxeerXStatus;
  side: PaxeerXActivitySide;
  timestamp: string | null;
  asset: PaxeerXAsset | null;
  amount: string | null;
  counterparty: string | null;
}

export interface PaxeerXUnifiedAccount {
  identities: PaxeerXIdentities;
  balances: Array<PaxeerXBalance>;
  activity: Array<PaxeerXActivityItem>;
}

// Precompile probe result: which unified surfaces the connected node actually answers for.
export interface PaxeerXCapabilities {
  addr: boolean;
  custody: boolean;
  anchor: boolean;
  exchange: boolean;
  bridge: boolean;
  launchpad: boolean;
}
