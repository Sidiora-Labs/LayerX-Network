export interface TxActionGeneral {
  type: 'mint' | 'burn' | 'collect' | 'swap';
  data: {
    amount0: string;
    symbol0: string;
    address0: string;
    amount1: string;
    symbol1: string;
    address1: string;
  };
}

export interface TxActionNft {
  type: 'mint_nft';
  data: {
    name: string;
    symbol: string;
    address: string;
    to: string;
    ids: Array<string>;
  };
}

export type TxAction = {
  protocol: 'uniswap_v3';
} & (TxActionGeneral | TxActionNft);
