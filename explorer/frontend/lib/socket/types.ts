import type { Channel } from 'phoenix';

import type * as multichain from '@blockscout/multichain-aggregator-types';
import type * as zetaChainCCTXType from '@blockscout/zetachain-cctx-types';
import type { AddressCoinBalanceHistoryItem, AddressTokensBalancesSocketMessage } from 'types/api/address';
import type { NewArbitrumBatchSocketResponse } from 'types/api/arbitrumL2';
import type { NewBlockSocketResponse } from 'types/api/block';
import type { SmartContractVerificationResponse } from 'types/api/contract';
import type { RawTracesResponse } from 'types/api/rawTrace';
import type { TokenInstanceMetadataSocketMessage } from 'types/api/token';
import type { TokenTransfer } from 'types/api/tokenTransfer';
import type { Transaction } from 'types/api/transaction';
import type { NewZkEvmBatchSocketResponse } from 'types/api/zkEvmL2';

export type SocketMessageParams = SocketMessage.NewBlock |
SocketMessage.NewBlockMultichain |
SocketMessage.BlocksIndexStatus |
SocketMessage.InternalTxsIndexStatus |
SocketMessage.TxStatusUpdate |
SocketMessage.TxRawTrace |
SocketMessage.NewTx |
SocketMessage.NewInteropMessage |
SocketMessage.NewPendingTx |
SocketMessage.NewOptimisticDeposits |
SocketMessage.NewArbitrumDeposits |
SocketMessage.AddressBalance |
SocketMessage.AddressCurrentCoinBalance |
SocketMessage.AddressTokenBalance |
SocketMessage.AddressTokenBalancesErc20 |
SocketMessage.AddressTokenBalancesErc721 |
SocketMessage.AddressTokenBalancesErc1155 |
SocketMessage.AddressTokenBalancesErc404 |
SocketMessage.AddressCoinBalance |
SocketMessage.AddressTxs |
SocketMessage.AddressTxsPending |
SocketMessage.AddressTokenTransfer |
SocketMessage.AddressChangedBytecode |
SocketMessage.AddressFetchedBytecode |
SocketMessage.EthBytecodeDbLookupStarted |
SocketMessage.SmartContractWasVerified |
SocketMessage.SmartContractWasNotVerified |
SocketMessage.TokenTransfers |
SocketMessage.TokenTotalSupply |
SocketMessage.TokenInstanceMetadataFetched |
SocketMessage.ContractVerification |
SocketMessage.NewZkEvmL2Batch |
SocketMessage.NewArbitrumL2Batch |
SocketMessage.NewZetaChainCCTXs |
SocketMessage.Unknown;

interface SocketMessageParamsGeneric<Event extends string | undefined, Payload extends object | unknown> {
  channel: Channel | undefined;
  event: Event;
  handler: (payload: Payload) => void;
}

export namespace SocketMessage {
  export type NewBlock = SocketMessageParamsGeneric<'new_block', NewBlockSocketResponse>;
  export type NewBlockMultichain = SocketMessageParamsGeneric<'new_blocks', Array<{ block_number: number; chain_id: number }>>;
  export type BlocksIndexStatus = SocketMessageParamsGeneric<'index_status', { finished: boolean; ratio: string }>;
  export type InternalTxsIndexStatus = SocketMessageParamsGeneric<'index_status', { finished: boolean; ratio: string }>;
  export type TxStatusUpdate = SocketMessageParamsGeneric<'collated', NewBlockSocketResponse>;
  export type TxRawTrace = SocketMessageParamsGeneric<'raw_trace', RawTracesResponse>;
  export type NewTx = SocketMessageParamsGeneric<'transaction', { transaction: number }>;
  export type NewInteropMessage = SocketMessageParamsGeneric<'new_messages', Array<multichain.InteropMessage>>;
  export type NewPendingTx = SocketMessageParamsGeneric<'pending_transaction', { pending_transaction: number }>;
  export type NewOptimisticDeposits = SocketMessageParamsGeneric<'new_optimism_deposits', { deposits: number }>;
  export type NewArbitrumDeposits = SocketMessageParamsGeneric<'new_messages_to_rollup_amount', { new_messages_to_rollup_amount: number }>;
  export type AddressBalance = SocketMessageParamsGeneric<'balance', { balance: string; block_number: number; exchange_rate: string }>;
  export type AddressCurrentCoinBalance =
  SocketMessageParamsGeneric<'current_coin_balance', { coin_balance: string; block_number: number; exchange_rate: string }>;
  export type AddressTokenBalance = SocketMessageParamsGeneric<'token_balance', { block_number: number }>;
  export type AddressTokenBalancesErc20 = SocketMessageParamsGeneric<'updated_token_balances_erc_20', AddressTokensBalancesSocketMessage>;
  export type AddressTokenBalancesErc721 = SocketMessageParamsGeneric<'updated_token_balances_erc_721', AddressTokensBalancesSocketMessage>;
  export type AddressTokenBalancesErc1155 = SocketMessageParamsGeneric<'updated_token_balances_erc_1155', AddressTokensBalancesSocketMessage>;
  export type AddressTokenBalancesErc404 = SocketMessageParamsGeneric<'updated_token_balances_erc_404', AddressTokensBalancesSocketMessage>;
  export type AddressCoinBalance = SocketMessageParamsGeneric<'coin_balance', { coin_balance: AddressCoinBalanceHistoryItem }>;
  export type AddressTxs = SocketMessageParamsGeneric<'transaction', { transactions: Array<Transaction> }>;
  export type AddressTxsPending = SocketMessageParamsGeneric<'pending_transaction', { transactions: Array<Transaction> }>;
  export type AddressTokenTransfer = SocketMessageParamsGeneric<'token_transfer', { token_transfers: Array<TokenTransfer> }>;
  export type AddressChangedBytecode = SocketMessageParamsGeneric<'changed_bytecode', Record<string, never>>;
  export type AddressFetchedBytecode = SocketMessageParamsGeneric<'fetched_bytecode', { fetched_bytecode: string }>;
  export type EthBytecodeDbLookupStarted = SocketMessageParamsGeneric<'eth_bytecode_db_lookup_started', Record<string, never>>;
  export type SmartContractWasVerified = SocketMessageParamsGeneric<'smart_contract_was_verified', Record<string, never>>;
  export type SmartContractWasNotVerified = SocketMessageParamsGeneric<'smart_contract_was_not_verified', Record<string, never>>;
  export type TokenTransfers = SocketMessageParamsGeneric<'token_transfer', { token_transfer: number }>;
  export type TokenTotalSupply = SocketMessageParamsGeneric<'total_supply', { total_supply: number }>;
  export type TokenInstanceMetadataFetched = SocketMessageParamsGeneric<'fetched_token_instance_metadata', TokenInstanceMetadataSocketMessage>;
  export type ContractVerification = SocketMessageParamsGeneric<'verification_result', SmartContractVerificationResponse>;
  export type NewZkEvmL2Batch = SocketMessageParamsGeneric<'new_zkevm_confirmed_batch', NewZkEvmBatchSocketResponse>;
  export type NewArbitrumL2Batch = SocketMessageParamsGeneric<'new_arbitrum_batch', NewArbitrumBatchSocketResponse>;
  export type NewZetaChainCCTXs = SocketMessageParamsGeneric<'new_cctxs', Array<zetaChainCCTXType.CctxListItem>>;
  export type Unknown = SocketMessageParamsGeneric<undefined, unknown>;
}
