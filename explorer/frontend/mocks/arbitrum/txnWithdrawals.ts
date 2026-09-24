import type { ArbitrumL2TxnWithdrawalsItem } from 'types/api/arbitrumL2';

export const unclaimed: ArbitrumL2TxnWithdrawalsItem = {
  arb_block_number: 115114348,
  caller_address_hash: '0x07e1e36fe70cd58a05c00812d573dc39a127ee6d',
  callvalue: '21000000000000000000',
  completion_transaction_hash: null,
  data: '0x',
  destination_address_hash: '0x07e1e36fe70cd58a05c00812d573dc39a127ee6d',
  eth_block_number: 7503173,
  id: 59874,
  l2_timestamp: 1737020350,
  status: 'confirmed',
  token: null,
};

export const claimed: ArbitrumL2TxnWithdrawalsItem = {
  arb_block_number: 115114348,
  caller_address_hash: '0x07e1e36fe70cd58a05c00812d573dc39a127ee6d',
  callvalue: '21000000000000000000',
  completion_transaction_hash: '0x215382498438cb6532a5e5fb07d664bbf912187866591470d47c3cfbce2dc4a8',
  data: '0x',
  destination_address_hash: '0x07e1e36fe70cd58a05c00812d573dc39a127ee6d',
  eth_block_number: 7503173,
  id: 59875,
  l2_timestamp: 1737020350,
  status: 'relayed',
  token: {
    address_hash: '0x0000000000000000000000000000000000000000',
    symbol: 'USDC',
    name: 'USDC Token',
    decimals: 6,
    amount: '10000000000',
    destination_address_hash: '0x07e1e36fe70cd58a05c00812d573dc39a127ee6d',
  },
};
