import type { PaxeerXUnifiedAccount } from 'types/api/paxeerX';

const PLACEHOLDER_HASH = '0x0000000000000000000000000000000000000000000000000000000000000000';

// Shape-only data that drives the skeletons while the unified account request is in flight.
export const UNIFIED_ACCOUNT_PLACEHOLDER: PaxeerXUnifiedAccount = {
  identities: {
    evm: '0x0000000000000000000000000000000000000000',
    pax: 'pax1000000000000000000000000000000000000000',
    did: `did:layerx:${ '0'.repeat(64) }`,
    kernel_account: `agent:did:layerx:${ '0'.repeat(64) }:main`,
  },
  balances: Array.from({ length: 3 }, (_, index) => ({
    asset: {
      id: `placeholder-${ index }`,
      denom: 'HPX',
      symbol: 'HPX',
      decimals: 18,
    },
    total: '0',
    parts: {
      chain: '0',
      custody: '0',
      kernel: '0',
    },
  })),
  activity: Array.from({ length: 5 }, (_, index) => ({
    kind: 'custody_deposit',
    hash: PLACEHOLDER_HASH,
    block_number: index,
    status: 'instant' as const,
    side: 'kernel' as const,
    timestamp: null,
    asset: null,
    amount: null,
    counterparty: null,
  })),
};
