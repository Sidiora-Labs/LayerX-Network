import type { Chain, GetTransactionReturnType, TransactionReceipt } from 'viem';

import type { Transaction } from 'types/api/transaction';

import dayjs from 'lib/date/dayjs';
import hexToDecimal from 'lib/hexToDecimal';
import { unknownAddress } from 'ui/shared/address/utils';

export default function formatTxData(
  tx: GetTransactionReturnType<Chain, 'latest'>,
  receipt: TransactionReceipt | null,
  confirmations: bigint | null,
  block: { timestamp: bigint | null; baseFeePerGas: bigint | null | undefined } | null,
): Transaction | null {
  const status = (() => {
    if (!receipt) {
      return null;
    }

    return receipt.status === 'success' ? 'ok' : 'error';
  })();

  const gasPrice = receipt?.effectiveGasPrice ?? tx.gasPrice;

  return {
    from: { ...unknownAddress, hash: tx.from as string },
    to: tx.to ? { ...unknownAddress, hash: tx.to as string } : null,
    hash: tx.hash as string,
    timestamp: block?.timestamp ? dayjs.unix(Number(block.timestamp)).format() : null,
    confirmation_duration: null,
    status,
    block_number: tx.blockNumber ? Number(tx.blockNumber) : null,
    value: tx.value.toString(),
    gas_price: gasPrice?.toString() ?? null,
    base_fee_per_gas: block?.baseFeePerGas?.toString() ?? null,
    max_fee_per_gas: tx.maxFeePerGas?.toString() ?? null,
    max_priority_fee_per_gas: tx.maxPriorityFeePerGas?.toString() ?? null,
    nonce: tx.nonce,
    position: tx.transactionIndex,
    type: tx.typeHex ? hexToDecimal(tx.typeHex) : null,
    raw_input: tx.input,
    gas_used: receipt?.gasUsed?.toString() ?? null,
    gas_limit: tx.gas.toString(),
    confirmations: confirmations && confirmations > 0 ? Number(confirmations) : 0,
    fee: {
      value: receipt && gasPrice ? (receipt.gasUsed * gasPrice).toString() : null,
      type: 'actual',
    },
    created_contract: receipt?.contractAddress ?
      { ...unknownAddress, hash: receipt.contractAddress, is_contract: true } :
      null,
    result: '',
    priority_fee: null,
    transaction_burnt_fee: null,
    revert_reason: null,
    decoded_input: null,
    has_error_in_internal_transactions: null,
    token_transfers: null,
    token_transfers_overflow: false,
    exchange_rate: null,
    historic_exchange_rate: null,
    method: null,
    transaction_types: [],
    transaction_tag: null,
    actions: [],
  };
}
