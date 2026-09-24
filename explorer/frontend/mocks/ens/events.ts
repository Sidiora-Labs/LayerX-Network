import type * as bens from '@blockscout/bens-types';

export const ensDomainEventA: bens.DomainEvent = {
  transaction_hash: '0x86c66b9fad66e4f20d42a6eed4fe12a0f48a274786fd85e9d4aa6c60e84b5874',
  timestamp: '2021-06-27T13:34:44.000000Z',
  from_address: {
    hash: '0xaa96a50a2f67111262fe24576bd85bb56ec65016',
  },
  action: '0xf7a16963',
};

export const ensDomainEventB: bens.DomainEvent = {
  transaction_hash: '0x150bf7d5cd42457dd9c799ddd9d4bf6c30c703e1954a88c6d4b668b23fe0fbf8',
  timestamp: '2022-11-02T14:20:24.000000Z',
  from_address: {
    hash: '0xfe6ab8a0dafe7d41adf247c210451c264155c9b0',
  },
  action: 'register',
};
