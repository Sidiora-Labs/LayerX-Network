import type { AddressParam } from 'types/api/addressParams';
import type { WithdrawalsResponse } from 'types/api/withdrawals';

export const data: WithdrawalsResponse = {
  items: [
    {
      amount: '192175000000000',
      block_number: 43242,
      index: 11688,
      receiver: {
        hash: '0xf97e180c050e5Ab072211Ad2C213Eb5AEE4DF134',
        implementations: null,
        is_contract: false,
        is_verified: null,
        name: null,
      } as AddressParam,
      timestamp: '2022-06-07T18:12:24.000000Z',
      validator_index: 49622,
    },
    {
      amount: '192175000000000',
      block_number: 43242,
      index: 11687,
      receiver: {
        hash: '0xf97e987c050e5Ab072211Ad2C213Eb5AEE4DF134',
        implementations: null,
        is_contract: false,
        is_verified: null,
        name: null,
      } as AddressParam,
      timestamp: '2022-05-07T18:12:24.000000Z',
      validator_index: 49621,
    },
    {
      amount: '182773000000000',
      block_number: 43242,
      index: 11686,
      receiver: {
        hash: '0xf97e123c050e5Ab072211Ad2C213Eb5AEE4DF134',
        implementations: null,
        is_contract: false,
        is_verified: null,
        name: null,
      } as AddressParam,
      timestamp: '2022-04-07T18:12:24.000000Z',
      validator_index: 49620,
    },
  ],
  next_page_params: {
    index: 11639,
    items_count: 50,
  },
};
