import type { PaxeerXAsset, PaxeerXUnifiedAccount } from 'types/api/paxeerX';

export const evmAddress = '0xd789a607CEac2f0E14867de4EB15b15C9FFB5859';
export const paxAddress = 'pax1qypqxpq9qcrsszg2pvxq6rs0zqg3yyc5z5tpwxqergd3c8g7rusqvyctmn';
export const did = 'did:layerx:9f2c1b4d5e6a7b8c9d0e1f2a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e';
export const kernelAccount = 'agent:did:layerx:9f2c1b4d5e6a7b8c9d0e1f2a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e:main';

export const nativeAsset: PaxeerXAsset = {
  id: '0x0000000000000000000000000000000000000000000000000000000000000001',
  denom: 'uhpx',
  symbol: 'HPX',
  decimals: 18,
};

export const pointerAsset: PaxeerXAsset = {
  id: '0x0000000000000000000000000000000000000000000000000000000000000002',
  denom: 'factory/pax1pointer/usdx',
  symbol: 'USDX',
  decimals: 6,
};

export const unifiedAccount: PaxeerXUnifiedAccount = {
  identities: {
    evm: evmAddress,
    pax: paxAddress,
    did,
    kernel_account: kernelAccount,
  },
  balances: [
    {
      asset: nativeAsset,
      total: '3000000000000000000',
      parts: {
        chain: '1000000000000000000',
        custody: '1500000000000000000',
        kernel: '500000000000000000',
      },
    },
    {
      asset: pointerAsset,
      total: '2500000',
      parts: {
        chain: '2500000',
        custody: '0',
        kernel: '0',
      },
    },
  ],
  activity: [
    {
      kind: 'custody-deposit',
      hash: '0x62d597ebcf3e8d60096dd0363bc2f0f5e2df27d1c9b95cc51f1d9fb69f23c1a5',
      block_number: 1_284_017,
      status: 'final',
      side: 'chain',
      timestamp: '2024-04-02T10:12:35.000000Z',
      asset: nativeAsset,
      amount: '1500000000000000000',
      counterparty: '0x1013000000000000000000000000000000000000',
    },
    {
      kind: 'claim-queued',
      hash: '0x1f0e9c7a3b5d2e4f6081a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f7',
      block_number: 1_284_912,
      status: 'sealed',
      side: 'chain',
      timestamp: '2024-04-02T11:03:11.000000Z',
      asset: pointerAsset,
      amount: '2500000',
      counterparty: null,
    },
    {
      kind: 'replay_receipt',
      hash: '0x8a7b6c5d4e3f2a1b0c9d8e7f6a5b4c3d2e1f00112233445566778899aabbccdd',
      block_number: 1_285_004,
      status: 'instant',
      side: 'kernel',
      timestamp: null,
      asset: null,
      amount: null,
      counterparty: null,
    },
  ],
};

export const unifiedAccountEvmOnly: PaxeerXUnifiedAccount = {
  identities: {
    evm: evmAddress,
    pax: null,
    did: null,
    kernel_account: null,
  },
  balances: [],
  activity: [],
};
