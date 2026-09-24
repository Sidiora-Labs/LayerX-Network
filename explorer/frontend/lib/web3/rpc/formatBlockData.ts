import type { Chain, GetBlockReturnType } from 'viem';

import type { Block } from 'types/api/block';

import dayjs from 'lib/date/dayjs';
import { unknownAddress } from 'ui/shared/address/utils';

export default function formatBlockData(block: GetBlockReturnType<Chain, false, 'latest'> | null): Block | null {
  if (!block) {
    return null;
  }

  return {
    height: Number(block.number),
    timestamp: dayjs.unix(Number(block.timestamp)).format(),
    transactions_count: block.transactions.length,
    internal_transactions_count: 0,
    miner: { ...unknownAddress, hash: block.miner },
    size: Number(block.size),
    hash: block.hash,
    parent_hash: block.parentHash,
    difficulty: block.difficulty?.toString() ?? null,
    total_difficulty: block.totalDifficulty?.toString() ?? null,
    gas_used: block.gasUsed.toString(),
    gas_limit: block.gasLimit.toString(),
    nonce: block.nonce,
    base_fee_per_gas: block.baseFeePerGas?.toString() ?? null,
    burnt_fees: null,
    priority_fee: null,
    extra_data: block.extraData,
    state_root: block.stateRoot,
    gas_target_percentage: null,
    gas_used_percentage: null,
    burnt_fees_percentage: null,
    type: 'block', // we can't get this type from RPC, so it will always be a regular block
    transaction_fees: null,
    uncles_hashes: block.uncles,
    withdrawals_count: block.withdrawals?.length,
  };
}
