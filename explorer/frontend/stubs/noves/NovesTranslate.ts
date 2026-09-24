import type { NovesResponseData, NovesClassificationData, NovesRawTransactionData } from 'types/api/noves';

const NOVES_TRANSLATE_CLASSIFIED: NovesClassificationData = {
  description: 'Sent 0.04 ETH',
  received: [ {
    action: 'Sent Token',
    actionFormatted: 'Sent Token',
    amount: '45',
    from: { name: '', address: '0xa0393A76b132526a70450273CafeceB45eea6dEE' },
    to: { name: '', address: '0xa0393A76b132526a70450273CafeceB45eea6dEE' },
    token: {
      address: '',
      name: 'ETH',
      symbol: 'ETH',
      decimals: 18,
    },
  } ],
  sent: [],
  source: {
    type: '',
  },
  type: '0x2',
  typeFormatted: 'Send NFT',
};

const NOVES_TRANSLATE_RAW: NovesRawTransactionData = {
  blockNumber: 1,
  fromAddress: '0xCFC123a23dfeD71bDAE054e487989d863C525C73',
  gas: 2,
  gasPrice: 3,
  timestamp: 20000,
  toAddress: '0xCFC123a23dfeD71bDAE054e487989d863C525C73',
  transactionFee: 2,
  transactionHash: '0x128b79937a0eDE33258992c9668455f997f1aF24',
};

export const NOVES_TRANSLATE: NovesResponseData = {
  accountAddress: '0x2b824349b320cfa72f292ab26bf525adb00083ba9fa097141896c3c8c74567cc',
  chain: 'base',
  txTypeVersion: 2,
  rawTransactionData: NOVES_TRANSLATE_RAW,
  classificationData: NOVES_TRANSLATE_CLASSIFIED,
};
