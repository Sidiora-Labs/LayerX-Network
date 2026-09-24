import type { Address } from 'types/api/address';
import type { AddressParam } from 'types/api/addressParams';

import { publicTag, privateTag, watchlistName } from 'mocks/address/tag';
import { tokenInfo } from 'mocks/tokens/tokenInfo';

export const hash = '0xd789a607CEac2f0E14867de4EB15b15C9FFB5859';

export const withName: AddressParam = {
  hash: hash,
  implementations: null,
  is_contract: false,
  is_verified: null,
  name: 'ArianeeStore',
  private_tags: [],
  watchlist_names: [],
  public_tags: [],
  ens_domain_name: null,
};

export const withEns: AddressParam = {
  hash: hash,
  implementations: null,
  is_contract: false,
  is_verified: null,
  name: 'ArianeeStore',
  private_tags: [],
  watchlist_names: [],
  public_tags: [],
  ens_domain_name: 'kitty.kitty.kitty.cat.eth',
};

export const withNameTag: AddressParam = {
  hash: hash,
  implementations: null,
  is_contract: false,
  is_verified: null,
  name: 'ArianeeStore',
  private_tags: [],
  watchlist_names: [],
  public_tags: [],
  ens_domain_name: 'kitty.kitty.kitty.cat.eth',
  metadata: {
    reputation: null,
    tags: [
      { tagType: 'name', name: 'Mrs. Duckie', slug: 'mrs-duckie', ordinal: 0, meta: null },
    ],
  },
};

export const withoutName: AddressParam = {
  hash: hash,
  implementations: null,
  is_contract: false,
  is_verified: null,
  name: null,
  private_tags: [],
  watchlist_names: [],
  public_tags: [],
  ens_domain_name: null,
};

export const delegated: AddressParam = {
  ...withoutName,
  is_verified: true,
  proxy_type: 'eip7702',
};

export const token: Address = {
  hash: hash,
  implementations: null,
  is_contract: true,
  is_verified: false,
  name: null,
  private_tags: [],
  watchlist_names: [],
  watchlist_address_id: null,
  public_tags: [],
  token: tokenInfo,
  block_number_balance_updated_at: 8201413,
  coin_balance: '1',
  creation_transaction_hash: '0xc38cf7377bf72d6436f63c37b01b24d032101f20ec1849286dc703c712f10c98',
  creator_address_hash: '0x34A9c688512ebdB575e82C50c9803F6ba2916E72',
  creation_status: 'success',
  exchange_rate: '0.04311',
  has_logs: false,
  has_token_transfers: true,
  has_tokens: true,
  has_validated_blocks: false,
  ens_domain_name: null,
};

export const eoa: Address = {
  block_number_balance_updated_at: 30811263,
  coin_balance: '2782650189688719421432220500',
  creation_transaction_hash: '0xf2aff6501b632604c39978b47d309813d8a1bcca721864bbe86abf59704f195e',
  creator_address_hash: '0x803ad3F50b9e1fF68615e8B053A186e1be288943',
  creation_status: null,
  exchange_rate: '0.04311',
  has_logs: true,
  has_token_transfers: false,
  has_tokens: true,
  has_validated_blocks: false,
  hash: hash,
  implementations: [],
  is_contract: false,
  is_verified: false,
  name: null,
  private_tags: [ publicTag ],
  public_tags: [ privateTag ],
  token: null,
  watchlist_names: [ watchlistName ],
  watchlist_address_id: 42,
  ens_domain_name: null,
};

export const contract: Address = {
  block_number_balance_updated_at: 30811263,
  coin_balance: '27826501896887194214322205',
  creation_transaction_hash: '0xf2aff6501b632604c39978b47d309813d8a1bcca721864bbe86abf59704f195e',
  creator_address_hash: '0x803ad3F50b9e1fF68615e8B053A186e1be288943',
  creation_status: 'success',
  exchange_rate: '0.04311',
  has_logs: true,
  has_token_transfers: false,
  has_tokens: false,
  has_validated_blocks: false,
  hash: hash,
  implementations: [
    { address_hash: '0x2F4F4A52295940C576417d29F22EEb92B440eC89', name: 'HomeBridge' },
  ],
  is_contract: true,
  is_verified: true,
  name: 'EternalStorageProxy',
  private_tags: [ publicTag ],
  public_tags: [ privateTag ],
  token: null,
  watchlist_names: [ watchlistName ],
  watchlist_address_id: 42,
  ens_domain_name: null,
};

export const validator: Address = {
  block_number_balance_updated_at: 30811932,
  coin_balance: '22910462800601256910890',
  creation_transaction_hash: null,
  creator_address_hash: null,
  creation_status: null,
  exchange_rate: '0.00432018',
  has_logs: false,
  has_token_transfers: false,
  has_tokens: false,
  has_validated_blocks: true,
  hash: hash,
  implementations: [],
  is_contract: false,
  is_verified: false,
  name: 'Kiryl Ihnatsyeu',
  private_tags: [],
  public_tags: [],
  token: null,
  watchlist_names: [],
  watchlist_address_id: null,
  ens_domain_name: null,
};

export const filecoin = {
  ...validator,
  filecoin: {
    actor_type: 'evm' as const,
    id: 'f02977693',
    robust: 'f410fuiwj6a3yxajbohrl5vu6ns6o2e2jriul52lvzci',
  },
};
