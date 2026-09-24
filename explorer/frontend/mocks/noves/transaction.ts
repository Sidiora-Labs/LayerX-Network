import type { NovesResponseData } from 'types/api/noves';

import type { TokensData } from 'ui/tx/assetFlows/utils/getTokensData';

export const hash = '0x380400d04ebb4179a35b1d7fdef260776915f015e978f8587ef2704b843d4e53';

export const transaction: NovesResponseData = {
  accountAddress: '0xef6595A423c99f3f2821190A4d96fcE4DcD89a80',
  chain: 'eth-goerli',
  classificationData: {
    description: 'Called function \'stake\' on contract 0xef326CdAdA59D3A740A76bB5f4F88Fb2.',
    protocol: {
      name: null,
    },
    received: [],
    sent: [
      {
        action: 'sent',
        actionFormatted: 'Sent',
        amount: '3000',
        from: {
          address: '0xef6595A423c99f3f2821190A4d96fcE4DcD89a80',
          name: 'This wallet',
        },
        to: {
          address: '0xdD15D2650387Fb6FEDE27ae7392C402a393F8A37',
          name: null,
        },
        token: {
          address: '0x1bfe4298796198f8664b18a98640cec7c89b5baa',
          decimals: 18,
          name: 'PQR-Test',
          symbol: 'PQR',
        },
      },
      {
        action: 'paidGas',
        actionFormatted: 'Paid Gas',
        amount: '0.000395521502109448',
        from: {
          address: '0xef6595A423c99f3f2821190A4d96fcE4DcD89a80',
          name: 'This wallet',
        },
        to: {
          address: null,
          name: 'Validators',
        },
        token: {
          address: 'ETH',
          decimals: 18,
          name: 'ETH',
          symbol: 'ETH',
        },
      },
    ],
    source: {
      type: null,
    },
    type: 'unclassified',
    typeFormatted: 'Unclassified',
  },
  rawTransactionData: {
    blockNumber: 10388918,
    fromAddress: '0xef6595A423c99f3f2821190A4d96fcE4DcD89a80',
    gas: 275079,
    gasPrice: 1500000008,
    timestamp: 1705488588,
    toAddress: '0xef326CdAdA59D3A740A76bB5f4F88Fb2f1076164',
    transactionFee: {
      amount: '395521502109448',
      token: {
        decimals: 18,
        symbol: 'ETH',
      },
    },
    transactionHash: '0x380400d04ebb4179a35b1d7fdef260776915f015e978f8587ef2704b843d4e53',
  },
  txTypeVersion: 2,
};

export const tokenData: TokensData = {
  nameList: [ 'PQR-Test', 'ETH' ],
  symbolList: [ 'PQR' ],
  idList: [],
  byName: {
    'PQR-Test': {
      name: 'PQR-Test',
      symbol: 'PQR',
      address: '0x1bfe4298796198f8664b18a98640cec7c89b5baa',
      id: undefined,
    },
    ETH: { name: 'ETH', symbol: null, address: '', id: undefined },
  },
  bySymbol: {
    PQR: {
      name: 'PQR-Test',
      symbol: 'PQR',
      address: '0x1bfe4298796198f8664b18a98640cec7c89b5baa',
      id: undefined,
    },
    'null': { name: 'ETH', symbol: null, address: '', id: undefined },
  },
};
