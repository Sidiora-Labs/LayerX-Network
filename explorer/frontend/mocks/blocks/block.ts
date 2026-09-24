/* eslint-disable max-len */
import type { RpcBlock } from 'viem';

import type { Block, BlocksResponse, ZilliqaBlockData } from 'types/api/block';

import { ZERO_ADDRESS } from 'toolkit/utils/consts';

import * as addressMock from '../address/address';
import * as tokenMock from '../tokens/tokenInfo';

export const base: Block = {
  base_fee_per_gas: '10000000000',
  burnt_fees: '5449200000000000',
  burnt_fees_percentage: 20.292245650793845,
  difficulty: '340282366920938463463374607431768211454',
  extra_data: 'TODO',
  gas_limit: '12500000',
  gas_target_percentage: -91.28128,
  gas_used: '544920',
  gas_used_percentage: 4.35936,
  hash: '0xccc75136de485434d578b73df66537c06b34c3c9b12d085daf95890c914fc2bc',
  height: 30146364,
  miner: {
    hash: '0xdAd49e6CbDE849353ab27DeC6319E687BFc91A41',
    implementations: null,
    is_contract: false,
    is_verified: null,
    name: 'Alex Emelyanov',
    private_tags: [],
    public_tags: [],
    watchlist_names: [],
    ens_domain_name: null,
  },
  nonce: '0x0000000000000000',
  parent_hash: '0x44125f0eb36a9d942e0c23bb4e8117f7ba86a9537a69b59c0025986ed2b7500f',
  priority_fee: '23211757500000000',
  rewards: [
    {
      reward: '500000000000000000',
      type: 'POA Mania Reward',
    },
    {
      reward: '1026853607510000000',
      type: 'Validator Reward',
    },
    {
      reward: '500000000000000000',
      type: 'Emission Reward',
    },
  ],
  size: 2448,
  state_root: 'TODO',
  timestamp: '2022-11-11T11:59:35Z',
  total_difficulty: '10258276095980170141167591583995189665817672619',
  transactions_count: 5,
  internal_transactions_count: 12,
  transaction_fees: '26853607500000000',
  type: 'block',
  uncles_hashes: [],
};

export const genesis: Block = {
  base_fee_per_gas: null,
  burnt_fees: null,
  burnt_fees_percentage: null,
  difficulty: '131072',
  extra_data: 'TODO',
  gas_limit: '6700000',
  gas_target_percentage: -100,
  gas_used: '0',
  gas_used_percentage: 0,
  hash: '0x39f02c003dde5b073b3f6e1700fc0b84b4877f6839bb23edadd3d2d82a488634',
  height: 0,
  miner: {
    hash: '0x0000000000000000000000000000000000000000',
    implementations: null,
    is_contract: false,
    is_verified: null,
    name: null,
    private_tags: [],
    public_tags: [],
    watchlist_names: [],
    ens_domain_name: 'kitty.kitty.cat.eth',
  },
  nonce: '0x0000000000000000',
  parent_hash: '0x0000000000000000000000000000000000000000000000000000000000000000',
  priority_fee: null,
  rewards: [],
  size: 533,
  state_root: 'TODO',
  timestamp: '2017-12-16T00:13:24.000000Z',
  total_difficulty: '131072',
  transactions_count: 0,
  internal_transactions_count: 0,
  transaction_fees: '0',
  type: 'block',
  uncles_hashes: [],
};

export const base2: Block = {
  ...base,
  height: base.height - 1,
  size: 592,
  miner: {
    hash: '0xDfE10D55d9248B2ED66f1647df0b0A46dEb25165',
    implementations: null,
    is_contract: false,
    is_verified: null,
    name: 'Kiryl Ihnatsyeu',
    private_tags: [],
    public_tags: [],
    watchlist_names: [],
    ens_domain_name: null,
  },
  timestamp: '2022-11-11T11:46:05Z',
  transactions_count: 253,
  gas_target_percentage: 23.6433,
  gas_used: '6333342',
  gas_used_percentage: 87.859504,
  burnt_fees: '232438000000000000',
  burnt_fees_percentage: 65.3333333333334,
  rewards: [
    {
      reward: '500000000000000000',
      type: 'Chore Reward',
    },
    {
      reward: '1017432850000000000',
      type: 'Miner Reward',
    },
    {
      reward: '500000000000000000',
      type: 'Emission Reward',
    },
  ],
  is_pending_update: true,
};

export const rootstock: Block = {
  ...base,
  bitcoin_merged_mining_coinbase_transaction: '0x0000000000000080a1219cea298d65d545b56abafe7c5421edfaf084cf9e374bb23ea985ebd86b206088ac0000000000000000266a24aa21a9edb2ac3022ad2a5327449f029b6aa3d2e55605061b5d8171b30abf5b330d1959c900000000000000002a6a52534b424c4f434b3a481d071e57c6c47cb8eb716295a7079b15859962abf35e32f107b21f003f0bb900000000',
  bitcoin_merged_mining_header: '0x000000204a7e42cadf8b5b0a094755c5a13298e596d61f361c6d31171a00000000000000970e51977cd6f82bab9ed62e678c8d8ca664af9d5c3b5cea39d5d4337c7abedae334c9649fc63e1982a84aaa',
  bitcoin_merged_mining_merkle_proof: '0x09f386e5e6feb20706a1b5d0817eae96f0ebb0d713eeefe6d5625afc6fd87fcdfe8cc9118bb49e32db87f8e928dcb13dd327b526ced76fb9de0115a5dca8d2a9657c929360ad07418fc7e1a3120da27e0002470d0c98c9b8b5b2835e64e379421d2469204533307bf0c5a087d93fd1dfb3aaea3ee83099928860f6cca891cf59d73c4e3c6053ea4b385dce39067e87c28805ddd89c4ff10500401bec7c248f749ad6f0933e6ad270e447d01711aca1cc26d7989ee59e1431fd2fd5d058edca6d',
  hash_for_merged_mining: '0x481d071e57c6c47cb8eb716295a7079b15859962abf35e32f107b21f003f0bb9',
  minimum_gas_price: '59240000',
};

export const celo: Block = {
  ...base,
  celo: {
    base_fee: {
      token: tokenMock.tokenInfoERC20a,
      amount: '445690000000000',
      breakdown: [
        {
          address: addressMock.withName,
          amount: '356552000000000.0000000000000',
          percentage: 80,
        },
        {
          address: {
            ...addressMock.withoutName,
            hash: ZERO_ADDRESS,
          },
          amount: '89138000000000.0000000000000',
          percentage: 20,
        },
      ],
      recipient: addressMock.contract,
    },
    epoch_number: 1486,
    l1_era_finalized_epoch_number: 1485,
  },
};

export const zilliqaWithAggregateQuorumCertificate: Block = {
  ...base,
  zilliqa: {
    view: 1137735,
    aggregate_quorum_certificate: {
      signature: '0x82d29e8f06adc890f6574c3d0ae0c811de1db695b05ed2755ef384fe21bc44f6505b99e201f6000a65f38ff6a13e286306d0e380ef1b43a273eb9947b3f11f852e14b93c258c32b516f89696fcb1190b147364b789572ebdf85d79c4cf3cbbbb',
      view: 1137735,
      signers: [ 1, 2, 3, 8 ],
      nested_quorum_certificates: [
        {
          signature: '0xaeb3567577f9db68565c6f97c158b17522620a9684c6f6beaa78920951ad4cae0f287b630bdd034c4a4f89ada42e3dbe012985e976a6f64057d735a4531a26b4e46c182414eabbe625e5b15e6645be5b6522bdec113df408874f6d1e0d894dca',
          view: 1137732,
          proposed_by_validator_index: 1,
          signers: [ 3, 8 ],
        },
        {
          signature: '0xaeb3567577f9db68565c6f97c158b17522620a9684c6f6beaa78920951ad4cae0f287b630bdd034c4a4f89ada42e3dbe012985e976a6f64057d735a4531a26b4e46c182414eabbe625e5b15e6645be5b6522bdec113df408874f6d1e0d894dca',
          view: 1137732,
          proposed_by_validator_index: 2,
          signers: [ 0, 2 ],
        },
      ],
    },
    quorum_certificate: {
      signature: '0xaeb3567577f9db68565c6f97c158b17522620a9684c6f6beaa78920951ad4cae0f287b630bdd034c4a4f89ada42e3dbe012985e976a6f64057d735a4531a26b4e46c182414eabbe625e5b15e6645be5b6522bdec113df408874f6d1e0d894dca',
      view: 1137732,
      signers: [ 0, 2, 3, 8 ],
    },
  },
};

export const zilliqaWithoutAggregateQuorumCertificate: Block = {
  ...base,
  zilliqa: {
    ...zilliqaWithAggregateQuorumCertificate.zilliqa,
    aggregate_quorum_certificate: null,
  } as ZilliqaBlockData,
};

export const withBlobTxs: Block = {
  ...base,
  blob_gas_price: '21518435987',
  blob_gas_used: '393216',
  burnt_blob_fees: '8461393325064192',
  excess_blob_gas: '79429632',
  blob_transactions_count: 1,
};

export const withWithdrawals: Block = {
  ...base,
  withdrawals_count: 2,
};

export const baseListResponse: BlocksResponse = {
  items: [
    base,
    base2,
  ],
  next_page_params: null,
};

export const rpcBlockBase: RpcBlock = {
  difficulty: '0x37fcc04bef8',
  extraData: '0x476574682f76312e302e312d38326566323666362f6c696e75782f676f312e34',
  gasLimit: '0x2fefd8',
  gasUsed: '0x0',
  hash: '0xfbafb4b7b6f6789338d15ff046f40dc608a42b1a33b093e109c6d7a36cd76f61',
  logsBloom: '0x0',
  miner: '0xe6a7a1d47ff21b6321162aea7c6cb457d5476bca',
  mixHash: '0x038956b9df89d0c1f980fd656d045e912beafa515cff7d7fd3c5f34ffdcb9e4b',
  nonce: '0xd8d3392f340bbb22',
  number: '0x1869f',
  parentHash: '0x576fd45e598c9f86835f50fe2c6e6d11df2d4c4b01f19e4241b7e793d852f9e4',
  receiptsRoot: '0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421',
  sha3Uncles: '0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347',
  size: '0x225',
  stateRoot: '0x32356228651d64cc5e6e7be87a556ecdbf40e876251dc867ba9e4bb82a0124a3',
  timestamp: '0x55d19741',
  totalDifficulty: '0x259e89748daae17',
  transactions: [
    '0x0e70849f10e22fe2e53fe6755f86a572aa6bb2fc472f0b87d9e561efa1fc2e1f',
    '0xae5624c77f06d0164301380afa7780ebe49debe77eb3d5167004d69bd188a09f',
  ],
  transactionsRoot: '0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421',
  uncles: [],
  baseFeePerGas: null,
  blobGasUsed: `0x0`,
  excessBlobGas: `0x0`,
  sealFields: [],
  withdrawals: [
    { address: '0xb9d7934878b5fb9610b3fe8a5e441e8fad7e293f', amount: '0x12128cd', index: '0x3216bbb', validatorIndex: '0x4dca3' },
    { address: '0xb9d7934878b5fb9610b3fe8a5e441e8fad7e293f', amount: '0x12027dd', index: '0x3216bbc', validatorIndex: '0x4dca4' },
  ],
};

export const rpcBlockWithTxsInfo: RpcBlock = {
  ...rpcBlockBase,
  transactions: [
    {
      accessList: [
        {
          address: '0x7af661a6463993e05a171f45d774cf37e761c83f',
          storageKeys: [
            '0x0000000000000000000000000000000000000000000000000000000000000007',
            '0x000000000000000000000000000000000000000000000000000000000000000c',
            '0x0000000000000000000000000000000000000000000000000000000000000008',
            '0x0000000000000000000000000000000000000000000000000000000000000006',
            '0x0000000000000000000000000000000000000000000000000000000000000009',
            '0x000000000000000000000000000000000000000000000000000000000000000a',
          ],
        },
        {
          address: '0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2',
          storageKeys: [
            '0x0d726f311404f8052d44e7004a6ffb747709a6d3666a62ce3f5aad13374680ab',
            '0x1a824a6850dcbd9223afea4418727593881e2911ed2e734272a263153159fe26',
            '0xfae3a383c82daf853bbd8bbcd21280410599b135c274c01354ea7d3a5e09f43c',
          ],
        },
      ],
      blockHash: '0xeb37ebc94e31773e5c5703073fd3911b2ab596f099d00d18b55ae3ac8203c1d5',
      blockNumber: '0x136058d',
      chainId: '0x1',
      from: '0x111527f1386c6725a2f5986230f3060bdcac041f',
      gas: '0xf4240',
      gasPrice: '0x1780b2ff9',
      hash: '0x0e70849f10e22fe2e53fe6755f86a572aa6bb2fc472f0b87d9e561efa1fc2e1f',
      input: '0x258d7af661a6463993e05a171f45d774cf37e761c83f402ab3277301b3574863a151d042dc870fb1b3f0c72cbbdd53a85898f62415fe124406f6608d8802269d1283cdb2a5a329649e5cb4cdcee91ab6',
      // maxFeePerGas: '0x3ac1bf7ee',
      // maxPriorityFeePerGas: '0x0',
      nonce: '0x127b2',
      r: '0x3c47223f880a3fb7b1eca368d9d7320d2278f0b679109d9ed0af4080ee386f23',
      s: '0x587a441f9472b312ff302d7132547aa250ea06c6203c76831d56a46ec188e664',
      to: '0x000000d40b595b94918a28b27d1e2c66f43a51d3',
      transactionIndex: '0x0',
      type: '0x1',
      v: '0x1',
      value: '0x31',
      yParity: '0x1',
    },
    {
      accessList: [],
      blockHash: '0xeb37ebc94e31773e5c5703073fd3911b2ab596f099d00d18b55ae3ac8203c1d5',
      blockNumber: '0x136058d',
      chainId: '0x1',
      from: '0xe25d2cb47b606bb6fd9272125457a7230e26f956',
      gas: '0x47bb0',
      gasPrice: '0x1ba875cb6',
      hash: '0xae5624c77f06d0164301380afa7780ebe49debe77eb3d5167004d69bd188a09f',
      input: '0x3593564c000000000000000000000000000000000000000000000000000000000000006000000000000000000000000000000000000000000000000000000000000000a0000000000000000000000000000000000000000000000000000000006696237b00000000000000000000000000000000000000000000000000000000000000040b080604000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000008000000000000000000000000000000000000000000000000000000000000000e0000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000002800000000000000000000000000000000000000000000000000000000000000040000000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000b1a2bc2ec500000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000b1a2bc2ec5000000000000000000000000000000000000000000000000000000006d1aaedfab0f00000000000000000000000000000000000000000000000000000000000000a000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000002000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2000000000000000000000000d84d4e8e1e8f268e027c29fa4d48c4b7e4d422990000000000000000000000000000000000000000000000000000000000000060000000000000000000000000d84d4e8e1e8f268e027c29fa4d48c4b7e4d42299000000000000000000000000000000fee13a103a10d593b9ae06b3e05f2e7e1c00000000000000000000000000000000000000000000000000000000000000190000000000000000000000000000000000000000000000000000000000000060000000000000000000000000d84d4e8e1e8f268e027c29fa4d48c4b7e4d42299000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000006cd4db3c8c8d',
      // maxFeePerGas: '0x23493c9cd',
      // maxPriorityFeePerGas: '0x427c2cbd',
      nonce: '0x32b',
      r: '0x6566181b3cfd01702b24a2124ea7698b8cc815c7f37d1ea55800f176ca7a94cf',
      s: '0x34f8dd837f37746ccd18f4fa71e05de98a2212f1c931f740598e491518616bb3',
      to: '0x3fc91a3afd70395cd496c647d5a6cc9d4b2b7fad',
      transactionIndex: '0x1',
      type: '0x1',
      v: '0x1',
      value: '0xb1a2bc2ec50000',
      yParity: '0x1',
    },
  ],
};
