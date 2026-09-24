import type { ChainInfo, InteropMessage } from 'types/api/interop';

export const chain: ChainInfo = {
  chain_id: 1,
  chain_name: 'Ethereum',
  chain_logo: 'https://example.com/logo.png',
  instance_url: 'https://example.com',
};

export const interopMessageIn: InteropMessage = {
  init_transaction_hash: '0x047A81aFB05D9B1f8844bf60fcA05DCCFbC584B9',
  nonce: 1,
  payload: 'payload',
  relay_transaction_hash: '0x62d597ebcf3e8d60096dd0363bc2f0f5e2df27ba1dacd696c51aa7c9409f3193',
  sender_address_hash: '0x047A81aFB05D9B1f8844bf60fcA05DCCFbC584B9',
  status: 'Relayed',
  target_address_hash: '0x047A81aFB05D9B1f8844bf60fcA05DCCFbC584B9',
  timestamp: '2022-10-10T14:34:30.000000Z',
  init_chain: chain,
};

export const interopMessageIn1: InteropMessage = {
  init_transaction_hash: '0x047A81aFB05D9B1f8844bf60fcA05DCCFbC584B9',
  nonce: 1,
  payload: 'payload',
  relay_transaction_hash: '0x62d597ebcf3e8d60096dd0363bc2f0f5e2df27ba1dacd696c51aa7c9409f3193',
  sender_address_hash: '0x047A81aFB05D9B1f8844bf60fcA05DCCFbC584B9',
  status: 'Sent',
  target_address_hash: '0x047A81aFB05D9B1f8844bf60fcA05DCCFbC584B9',
  timestamp: '2022-10-10T14:34:30.000000Z',
  init_chain: null,
};

export const interopMessageOut: InteropMessage = {
  init_transaction_hash: '0x047A81aFB05D9B1f8844bf60fcA05DCCFbC584B9',
  nonce: 1,
  payload: 'payload',
  relay_transaction_hash: '0x62d597ebcf3e8d60096dd0363bc2f0f5e2df27ba1dacd696c51aa7c9409f3193',
  sender_address_hash: '0x047A81aFB05D9B1f8844bf60fcA05DCCFbC584B9',
  status: 'Relayed',
  target_address_hash: '0x047A81aFB05D9B1f8844bf60fcA05DCCFbC584B9',
  timestamp: '2022-10-10T14:34:30.000000Z',
  relay_chain: chain,
};

export const interopMessageOut1: InteropMessage = {
  init_transaction_hash: '0x047A81aFB05D9B1f8844bf60fcA05DCCFbC584B9',
  nonce: 1,
  payload: 'payload',
  relay_transaction_hash: '0x62d597ebcf3e8d60096dd0363bc2f0f5e2df27ba1dacd696c51aa7c9409f3193',
  sender_address_hash: '0x047A81aFB05D9B1f8844bf60fcA05DCCFbC584B9',
  status: 'Failed',
  target_address_hash: '0x047A81aFB05D9B1f8844bf60fcA05DCCFbC584B9',
  timestamp: '2022-10-10T14:34:30.000000Z',
  relay_chain: null,
};
