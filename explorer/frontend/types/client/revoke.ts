import type { TokenReputation } from 'types/api/token';

export interface ContractAllowanceType {
  transactionId: `0x${ string }` | null;
  spender: `0x${ string }`;
  allowance?: bigint;
  blockNumber: bigint;
  tokenId?: string;
}

export interface AllowanceType {
  type: 'ERC-20' | 'ERC-721' | 'ERC-1155';
  address: `0x${ string }`;
  transactionId: `0x${ string }` | null;
  tokenId?: string;
  tokenIcon?: string;
  tokenReputation: TokenReputation | null;
  allowance?: string;
  price?: string;
  balance?: string;
  valueAtRiskUsd?: number;
  decimals?: number;
  spender: `0x${ string }`;
  spenderName?: string;
  symbol?: string;
  name?: string;
  totalSupply?: bigint;
  timestamp: number;
}
