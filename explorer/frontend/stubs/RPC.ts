import type { Chain, GetBlockReturnType, GetTransactionReturnType, TransactionReceipt, Withdrawal } from 'viem';

import { ADDRESS_HASH } from './addressParams';
import { BLOCK_HASH } from './block';
import { TX_HASH } from './tx';

export const WITHDRAWAL: Withdrawal = {
  index: '0x1af95d9',
  validatorIndex: '0x7d748',
  address: '0x9b52b9033ecbb6635f1c31a646d5691b282878aa',
  amount: '0x29e16b',
};

export const GET_TRANSACTION: GetTransactionReturnType<Chain, 'latest'> = {
  blockHash: BLOCK_HASH,
  blockNumber: BigInt(10361367),
  from: ADDRESS_HASH,
  gas: BigInt(800000),
  maxPriorityFeePerGas: BigInt(2),
  maxFeePerGas: BigInt(14),
  hash: TX_HASH,
  input: '0x7898e0',
  nonce: 117694,
  to: ADDRESS_HASH,
  transactionIndex: 60,
  value: BigInt(42),
  type: 'eip1559',
  accessList: [],
  chainId: 5,
  v: BigInt(0),
  r: '0x2c5022ff7f78a22f1a99afbd568f75cb52812189ed8c264c8310e0b8dba2c8a8',
  s: '0x50938f87c92b9eeb9777507ca8f7397840232d00d1dbac3edac6c115b4656763',
  yParity: 1,
  typeHex: '0x2',
};

export const GET_TRANSACTION_RECEIPT: TransactionReceipt = {
  blockHash: BLOCK_HASH,
  blockNumber: BigInt(10361367),
  contractAddress: null,
  cumulativeGasUsed: BigInt(39109),
  effectiveGasPrice: BigInt(13),
  from: ADDRESS_HASH,
  gasUsed: BigInt(39109),
  logs: [],
  logsBloom: '0x0',
  status: 'success',
  to: ADDRESS_HASH,
  transactionHash: TX_HASH,
  transactionIndex: 60,
  type: '0x2',
};

export const GET_TRANSACTION_CONFIRMATIONS = BigInt(420);

export const GET_BALANCE = BigInt(42_000_000_000_000);

export const GET_TRANSACTIONS_COUNT = 42;

export const GET_BLOCK: GetBlockReturnType<Chain, false, 'latest'> = {
  baseFeePerGas: BigInt(11),
  difficulty: BigInt(111),
  extraData: '0xd8830',
  gasLimit: BigInt(800000),
  gasUsed: BigInt(42000),
  hash: BLOCK_HASH,
  logsBloom: '0x008000',
  miner: ADDRESS_HASH,
  mixHash: BLOCK_HASH,
  nonce: '0x0000000000000000',
  number: BigInt(10361367),
  parentHash: BLOCK_HASH,
  receiptsRoot: BLOCK_HASH,
  sha3Uncles: BLOCK_HASH,
  size: BigInt(88),
  stateRoot: BLOCK_HASH,
  timestamp: BigInt(1628580000),
  totalDifficulty: BigInt(10361367),
  transactions: [
    TX_HASH,
  ],
  transactionsRoot: TX_HASH,
  uncles: [],
  withdrawals: Array(10).fill(WITHDRAWAL),
  withdrawalsRoot: TX_HASH,
  sealFields: [ '0x00' ],
  blobGasUsed: BigInt(0),
  excessBlobGas: BigInt(0),
};

export const GET_BLOCK_WITH_TRANSACTIONS: GetBlockReturnType<Chain, true, 'latest'> = {
  ...GET_BLOCK,
  transactions: Array(50).fill(GET_TRANSACTION),
};
