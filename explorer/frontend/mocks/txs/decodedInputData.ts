import type { DecodedInput } from 'types/api/decodedInput';

export const withoutIndexedFields: DecodedInput = {
  method_call: 'CreditSpended(uint256 _type, uint256 _quantity)',
  method_id: '58cdf94a',
  parameters: [
    {
      name: '_type',
      type: 'uint256',
      value: '3',
    },
    {
      name: '_quantity',
      type: 'uint256',
      value: '1',
    },
  ],
};

export const withIndexedFields: DecodedInput = {
  method_call: 'Transfer(address indexed from, address indexed to, uint256 value)',
  method_id: 'ddf252ad',
  parameters: [
    {
      indexed: true,
      name: 'from',
      type: 'address',
      value: '0xd789a607ceac2f0e14867de4eb15b15c9ffb5859',
    },
    {
      indexed: true,
      name: 'to',
      type: 'address',
      value: '0x7d20a8d54f955b4483a66ab335635ab66e151c51',
    },
    {
      indexed: false,
      name: 'value',
      type: 'uint256',
      value: '31567373703130350',
    },
    {
      indexed: true,
      name: 'inputArray',
      type: 'uint256[2][2]',
      value: [ [ '1', '1' ], [ '1', '1' ] ],
    },
  ],
};
