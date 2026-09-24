import type { GetAvailableBadgesResponse } from '@blockscout/points-types';

export const base: GetAvailableBadgesResponse = {
  items: [
    {
      chain_id: '17000',
      address: '0xf34B6A7d0BAbb5eBEe5521Ce7e5393A10782AE96',
      requirements: {
        streak: '30',
      },
      is_qualified: false,
      is_whitelisted: false,
      is_minted: false,
    },
    {
      chain_id: '17000',
      address: '0xf41583090c674E55d9755b0afcbbf9ea2FA378e7',
      requirements: {
        streak: '90',
      },
      is_qualified: false,
      is_whitelisted: false,
      is_minted: false,
    },
    {
      chain_id: '17000',
      address: '0x6a4676480C9E36652F62d4A751eeEf562E06a383',
      requirements: {
        streak: '180',
      },
      is_qualified: false,
      is_whitelisted: false,
      is_minted: false,
    },
  ],
};

export const filled: GetAvailableBadgesResponse = {
  items: [
    {
      chain_id: '17000',
      address: '0xf34B6A7d0BAbb5eBEe5521Ce7e5393A10782AE96',
      requirements: {
        streak: '30',
      },
      is_qualified: true,
      is_whitelisted: true,
      is_minted: true,
    },
    {
      chain_id: '17000',
      address: '0xf41583090c674E55d9755b0afcbbf9ea2FA378e7',
      requirements: {
        streak: '90',
      },
      is_qualified: true,
      is_whitelisted: true,
      is_minted: true,
    },
    {
      chain_id: '17000',
      address: '0x6a4676480C9E36652F62d4A751eeEf562E06a383',
      requirements: {
        streak: '180',
      },
      is_qualified: true,
      is_whitelisted: true,
      is_minted: false,
    },
  ],
};
