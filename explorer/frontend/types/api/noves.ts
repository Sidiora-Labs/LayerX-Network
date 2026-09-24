export interface NovesResponseData {
  txTypeVersion: number;
  chain: string;
  accountAddress: string;
  classificationData: NovesClassificationData;
  rawTransactionData: NovesRawTransactionData;
}

export interface NovesClassificationData {
  type: string;
  typeFormatted?: string;
  description: string;
  sent: Array<NovesSentReceived>;
  received: Array<NovesSentReceived>;
  approved?: Approved;
  protocol?: {
    name: string | null;
  };
  source: {
    type: string | null;
  };
  message?: string;
  deployedContractAddress?: string;
}

export interface Approved {
  amount: string;
  spender: string;
  token?: NovesToken;
  nft?: NovesNft;
}

export interface NovesSentReceived {
  action: string;
  actionFormatted?: string;
  amount: string;
  to: NovesTo;
  from: NovesFrom;
  token?: NovesToken;
  nft?: NovesNft;
}

export interface NovesToken {
  symbol: string;
  name: string;
  decimals: number;
  address: string;
  id?: string;
}

export interface NovesNft {
  name: string;
  id: string;
  symbol: string;
  address: string;
}

export interface NovesFrom {
  name: string | null;
  address: string;
}

export interface NovesTo {
  name: string | null;
  address: string | null;
}

export interface NovesRawTransactionData {
  transactionHash: string;
  fromAddress: string;
  toAddress: string;
  blockNumber: number;
  gas: number;
  gasPrice: number;
  transactionFee: NovesTransactionFee | number;
  timestamp: number;
}

export interface NovesTransactionFee {
  amount: string;
  currency?: string;
  token?: {
    decimals: number;
    symbol: string;
  };
}

export interface NovesAccountHistoryResponse {
  hasNextPage: boolean;
  items: Array<NovesResponseData>;
  pageNumber: number;
  pageSize: number;
  next_page_params?: {
    startBlock: string;
    endBlock: string;
    pageNumber: number;
    pageSize: number;
    ignoreTransactions: string;
    viewAsAccountAddress: string;
  };
}

export const NovesHistoryFilterValues = [ 'received', 'sent' ] as const;

export type NovesHistoryFilterValue = typeof NovesHistoryFilterValues[number] | undefined;

export interface NovesHistoryFilters {
  filter?: NovesHistoryFilterValue;
}

export interface NovesDescribeResponse {
  type: string;
  description: string;
}

export type NovesDescribeTxsResponse = {
  txHash: string;
  type: string;
  description: string;
};
