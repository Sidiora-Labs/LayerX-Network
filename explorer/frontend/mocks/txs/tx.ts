/* eslint-disable max-len */
import type { RpcTransactionReceipt } from 'viem';

import type { AddressParam } from 'types/api/addressParams';
import type { Transaction } from 'types/api/transaction';

import * as addressMock from 'mocks/address/address';
import { publicTag, privateTag, watchlistName } from 'mocks/address/tag';
import * as interopMock from 'mocks/interop/interop';
import { protocolTag } from 'mocks/metadata/address';
import * as tokenTransferMock from 'mocks/tokens/tokenTransfer';
import * as decodedInputDataMock from 'mocks/txs/decodedInputData';

export const base: Transaction = {
  base_fee_per_gas: '10000000000',
  block_number: 29611750,
  confirmation_duration: [
    0,
    6364,
  ],
  confirmations: 508299,
  created_contract: null,
  decoded_input: decodedInputDataMock.withoutIndexedFields,
  exchange_rate: '0.00254428',
  historic_exchange_rate: '0.00254428',
  fee: {
    type: 'actual',
    value: '7143168000000000',
  },
  from: {
    hash: '0x047A81aFB05D9B1f8844bf60fcA05DCCFbC584B9',
    implementations: null,
    is_contract: false,
    name: null,
    is_verified: null,
    private_tags: [ ],
    public_tags: [ publicTag ],
    watchlist_names: [],
    ens_domain_name: 'kitty.kitty.cat.eth',
  },
  gas_limit: '800000',
  gas_price: '48000000000',
  gas_used: '148816',
  hash: '0x62d597ebcf3e8d60096dd0363bc2f0f5e2df27ba1dacd696c51aa7c9409f3193',
  max_fee_per_gas: '40190625000',
  max_priority_fee_per_gas: '28190625000',
  method: 'updateSmartAsset',
  nonce: 27831,
  position: 7,
  priority_fee: '1299672384375000',
  raw_input: '0xfa4b78b90000000000000000000000000000000000000000000000000000000005001bcfe835d1028984e9e6e7d016b77164eacbcc6cc061e9333c0b37982b504f7ea791000000000000000000000000a79b29ad7e0196c95b87f4663ded82fbf2e3add8',
  result: 'success',
  revert_reason: null,
  status: 'ok',
  timestamp: '2022-10-10T14:34:30.000000Z',
  to: {
    hash: addressMock.hash,
    implementations: null,
    is_contract: false,
    is_verified: true,
    name: null,
    private_tags: [ privateTag ],
    public_tags: [],
    watchlist_names: [ watchlistName ],
    ens_domain_name: null,
  },
  token_transfers: [],
  token_transfers_overflow: false,
  transaction_burnt_fee: '461030000000000',
  transaction_tag: null,
  transaction_types: [
    'contract_call',
  ],
  type: 2,
  value: '42000000000000000000',
  actions: [],
  has_error_in_internal_transactions: false,
};

export const withWatchListNames: Transaction = {
  ...base,
  hash: '0x62d597ebcf3e8d60096dd0363bc2f0f5e2df27ba1dacd696c51aa7c9409f3194',
  from: {
    ...base.from,
    watchlist_names: [
      { label: 'from #1', display_name: 'from utka' },
      { label: 'kitty', display_name: 'kitty kitty kitty cat where are you' },
    ],
  },
  to: {
    ...base.to,
    watchlist_names: [ { label: 'to #1', display_name: 'to utka' } ],
  } as Transaction['to'],
};

export const withProtocolTag: Transaction = {
  ...base,
  hash: '0x62d597ebcf3e8d60096dd0363bc2f0f5e2df27ba1dacd696c51aa7c9409f3194',
  to: {
    ...(base.to as AddressParam),
    metadata: {
      tags: [ protocolTag ],
      reputation: null,
    },
    private_tags: [],
    watchlist_names: [],
    public_tags: [],
  },
};

export const withPendingUpdate: Transaction = {
  ...withProtocolTag,
  hash: '0x62d597ebcf3e8d60096dd0363bc2f0f5e2df27ba1dacd696c51aa7c9409f3133',
  is_pending_update: true,
};

export const withContractCreation: Transaction = {
  ...base,
  hash: '0x62d597ebcf3e8d60096dd0363bc2f0f5e2df27ba1dacd696c51aa7c9409f3195',
  to: null,
  created_contract: {
    hash: '0xdda21946FF3FAa027104b15BE6970CA756439F5a',
    implementations: null,
    is_contract: true,
    is_verified: null,
    name: 'Shavuha token',
    private_tags: [],
    public_tags: [],
    watchlist_names: [],
    ens_domain_name: null,
  },
  transaction_types: [
    'contract_creation',
  ],
};

export const withTokenTransfer: Transaction = {
  ...base,
  hash: '0x62d597ebcf3e8d60096dd0363bc2f0f5e2df27ba1dacd696c51aa7c9409f3196',
  to: {
    hash: addressMock.hash,
    implementations: null,
    is_contract: true,
    is_verified: true,
    name: 'ArianeeStore',
    private_tags: [ privateTag ],
    public_tags: [],
    watchlist_names: [ watchlistName ],
    ens_domain_name: null,
  },
  token_transfers: [
    tokenTransferMock.erc20,
    tokenTransferMock.erc721,
    tokenTransferMock.erc1155A,
    tokenTransferMock.erc1155B,
    tokenTransferMock.erc1155C,
    tokenTransferMock.erc1155D,
    tokenTransferMock.erc404A,
    tokenTransferMock.erc404B,
  ],
  token_transfers_overflow: true,
  transaction_types: [
    'token_transfer',
  ],
};

export const withDecodedRevertReason: Transaction = {
  ...base,
  status: 'error',
  result: 'Reverted',
  revert_reason: {
    method_call: 'SomeCustomError(address addr, uint256 balance)',
    method_id: '50289a9f',
    parameters: [
      {
        name: 'addr',
        type: 'address',
        value: '0xf26594f585de4eb0ae9de865d9053fee02ac6ef1',
      },
      {
        name: 'balance',
        type: 'uint256',
        value: '123',
      },
    ],
  },
};

export const withRawRevertReason: Transaction = {
  ...base,
  status: 'error',
  result: 'Reverted',
  revert_reason: {
    raw: '4f6e6c79206368616972706572736f6e2063616e206769766520726967687420746f20766f74652e',
  },
  to: {
    hash: addressMock.hash,
    implementations: null,
    is_verified: true,
    is_contract: true,
    name: 'Bad guy',
    private_tags: [ ],
    public_tags: [],
    watchlist_names: [ ],
    ens_domain_name: null,
  },
};

export const pending: Transaction = {
  ...base,
  base_fee_per_gas: null,
  block_number: null,
  confirmation_duration: [],
  confirmations: 0,
  decoded_input: null,
  gas_used: null,
  max_fee_per_gas: null,
  max_priority_fee_per_gas: null,
  method: null,
  position: null,
  priority_fee: null,
  result: 'pending',
  revert_reason: null,
  status: null,
  timestamp: null,
  transaction_burnt_fee: null,
  transaction_tag: null,
  type: null,
  value: '0',
};

export const withActionsUniswap: Transaction = {
  ...base,
  actions: [
    {
      data: {
        address0: '0x6f16598F00eDabEA92B4Cef4b6aa0d45c898A9AE',
        address1: '0xB4FBF271143F4FBf7B91A5ded31805e42b2208d6',
        amount0: '7143.488560357232097378',
        amount1: '10',
        symbol0: 'Ring ding ding daa baa Baa aramba baa bom baa barooumba Wh-wha-what&#39;s going on-on? Ding, ding This is the Crazy Frog Ding, ding Bem',
        symbol1: 'Ether',
      },
      protocol: 'uniswap_v3',
      type: 'mint',
    },
    {
      data: {
        address: '0xC36442b4a4522E871399CD717aBDD847Ab11FE88',
        ids: [
          '53699',
          '53700123456',
          '42',
        ],
        name: 'Uniswap V3: Positions NFT',
        symbol: 'UNI-V3-POS',
        to: '0x6d872Fb5F5B2B1f71fA9AadE159bc3976c1946B7',
      },
      protocol: 'uniswap_v3',
      type: 'mint_nft',
    },
    {
      data: {
        address0: '0x6f16598F00eDabEA92B4Cef4b6aa0d45c898A9AE',
        address1: '0xB4FBF271143F4FBf7B91A5ded31805e42b2208d6',
        amount0: '42876.488560357232',
        amount1: '345.908098203434',
        symbol0: 'SHAVUHA',
        symbol1: 'BOB',
      },
      protocol: 'uniswap_v3',
      type: 'swap',
    },
    {
      data: {
        address0: '0x6f16598F00eDabEA92B4Cef4b6aa0d45c898A9AE',
        address1: '0xB4FBF271143F4FBf7B91A5ded31805e42b2208d6',
        amount0: '42',
        amount1: '0.523523223232',
        symbol0: 'VIC',
        symbol1: 'USDT',
      },
      protocol: 'uniswap_v3',
      type: 'burn',
    },
    {
      data: {
        address0: '0x6f16598F00eDabEA92B4Cef4b6aa0d45c898A9AE',
        address1: '0xB4FBF271143F4FBf7B91A5ded31805e42b2208d6',
        amount0: '42',
        amount1: '0.523523223232',
        symbol0: 'BOB',
        symbol1: 'UNI',
      },
      protocol: 'uniswap_v3',
      type: 'collect',
    },
  ],
};

export const l2tx: Transaction = {
  ...base,
  l1_gas_price: '82702201886',
  l1_fee_scalar: '1.0',
  l1_gas_used: '17060',
  l1_fee: '1584574188135760',
  operator_fee: '2769347953',
};

export const stabilityTx: Transaction = {
  ...base,
  stability_fee: {
    dapp_address: {
      hash: '0xDc2B93f3291030F3F7a6D9363ac37757f7AD5C43',
      implementations: null,
      is_contract: false,
      is_verified: null,
      name: null,
      private_tags: [],
      public_tags: [],
      watchlist_names: [],
      ens_domain_name: null,
    },
    dapp_fee: '34381250000000',
    token: {
      address_hash: '0xDc2B93f3291030F3F7a6D9363ac37757f7AD5C43',
      circulating_market_cap: null,
      decimals: '18',
      exchange_rate: '123.567',
      holders_count: '92',
      icon_url: 'https://example.com/icon.png',
      name: 'Stability Gas',
      symbol: 'GAS',
      total_supply: '10000000000000000000000000',
      type: 'ERC-20',
      reputation: 'ok',
    },
    total_fee: '68762500000000',
    validator_address: {
      hash: '0x1432997a4058acbBe562F3c1E79738c142039044',
      implementations: null,
      is_contract: false,
      is_verified: null,
      name: null,
      private_tags: [],
      public_tags: [],
      watchlist_names: [],
      ens_domain_name: null,
    },
    validator_fee: '34381250000000',
  },
};

export const celoTxn: Transaction = {
  ...base,
  celo: {
    gas_token: {
      address_hash: '0x874069Fa1Eb16D44d622F2e0Ca25eeA172369bC1',
      circulating_market_cap: null,
      decimals: '18',
      exchange_rate: '0.42',
      holders_count: '205738',
      icon_url: 'https://example.com/icon.png',
      name: 'Celo Dollar',
      symbol: 'cUSD',
      total_supply: '7145754483836626799435133',
      type: 'ERC-20',
      reputation: 'ok',
    },
  },
};

export const arbitrumTxn: Transaction = {
  ...base,
  arbitrum: {
    batch_number: 743991,
    commitment_transaction: {
      hash: '0x71a25e01dde129a308704de217d200ea42e0f5b8c221c8ba8b2b680ff347f708',
      status: 'unfinalized',
      timestamp: '2024-11-19T14:26:23.000000Z',
    },
    confirmation_transaction: {
      hash: null,
      status: null,
      timestamp: null,
    },
    contains_message: null,
    gas_used_for_l1: '129773',
    gas_used_for_l2: '128313',
    message_related_info: {
      associated_l1_transaction_hash: null,
      message_status: 'Relayed',
    },
    network_fee: '1283130000000',
    poster_fee: '1297730000000',
    status: 'Sent to base',
  },
};

export const base2 = {
  ...base,
  hash: '0x02d597ebcf3e8d60096dd0363bc2f0f5e2df27ba1dacd696c51aa7c9409f3193',
  from: {
    ...base.from,
    hash: addressMock.hash,
  },
};

export const base3 = {
  ...base,
  hash: '0x12d597ebcf3e8d60096dd0363bc2f0f5e2df27ba1dacd696c51aa7c9409f3193',
  from: {
    ...base.from,
    hash: addressMock.hash,
  },
};

export const base4 = {
  ...base,
  hash: '0x22d597ebcf3e8d60096dd0363bc2f0f5e2df27ba1dacd696c51aa7c9409f3193',
};

export const withBlob = {
  ...base,
  blob_gas_price: '21518435987',
  blob_gas_used: '131072',
  blob_versioned_hashes: [
    '0x01a8c328b0370068aaaef49c107f70901cd79adcda81e3599a88855532122e09',
    '0x0197fdb17195c176b23160f335daabd4b6a231aaaadd73ec567877c66a3affd1',
  ],
  burnt_blob_fee: '2820464441688064',
  max_fee_per_blob_gas: '60000000000',
  transaction_types: [ 'blob_transaction' as const ],
  type: 3,
};

export const withRecipientName = {
  ...base,
  to: addressMock.withName,
};

export const withRecipientEns = {
  ...base,
  to: addressMock.withEns,
};

export const withRecipientNameTag = {
  ...withRecipientEns,
  to: addressMock.withNameTag,
};

export const withRecipientContract = {
  ...withRecipientEns,
  to: addressMock.contract,
};

export const withInteropInMessage: Transaction = {
  ...base,
  op_interop_messages: [ {
    init_chain: interopMock.chain,
    nonce: 1,
    payload: '0x',
    init_transaction_hash: '0x01a8c328b0370068aaaef49c107f70901cd79adcda81e3599a88855532122e09',
    sender_address_hash: addressMock.hash,
    status: 'Sent',
    target_address_hash: addressMock.hash,
  } ],
};

export const withInteropOutMessage: Transaction = {
  ...base,
  op_interop_messages: [ {
    relay_chain: interopMock.chain,
    nonce: 1,
    payload: '0xfa4b78b90000000000000000000000000000000000000000000000000000000005001bcfe835d1028984e9e6e7d016b77164eacbcc6cc061e9333c0b37982b504f7ea791000000000000000000000000a79b29ad7e0196c95b87f4663ded82fbf2e3add8',
    relay_transaction_hash: '0x01a8c328b0370068aaaef49c107f70901cd79adcda81e3599a88855532122e09',
    sender_address_hash: addressMock.hash,
    status: 'Sent',
    target_address_hash: addressMock.hash,
  } ],
};

export const rpcTxReceipt: RpcTransactionReceipt = {
  blockHash: '0xa737203aac9f38b5355c716f46b84ff1031335d1a99b2366900378c9e4c837a5',
  blockNumber: '0x171f82b',
  contractAddress: null,
  cumulativeGasUsed: '0xe235b',
  effectiveGasPrice: '0x793b22f4',
  from: '0x21dc71ddd3558cd7536bb5fa422303fb5559ea63',
  gasUsed: '0x215d2',
  logs: [],
  logsBloom: '0x00400040000000000000000000000000000000000000000010000000000000000000000020000000001000000000010400000000000000000000000000000000000400000000000000000008000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000010100000000000002801000000000000000000000000000000000000000000000000100000000004000000000000000080100000000000000000000000000000000000000000000802000000000000000000000000000000400000000000000000000000000000000000000000000000000000000004000000800000000000004000000000',
  status: 'success' as RpcTransactionReceipt['status'],
  to: '0xbd216513d74c8cf14cf4747e6aaa6420ff64ee9e',
  transactionHash: '0xc39cf2777f03346deba8659b3ff652bbcf3dcbd4fcf846a248a171e45cac94b2',
  transactionIndex: '0x2',
  type: '0x2',
};
