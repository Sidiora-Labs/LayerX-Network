import type { ValidatorZilliqa, ValidatorsZilliqaItem, ValidatorsZilliqaResponse } from 'types/api/validators';

export const validator1: ValidatorsZilliqaItem = {
  index: 420,
  bls_public_key: '0x95125dca41be848801f9bd75254f1faf1ae3194b1da53e9a5684ed7f67b729542482bc521924603b9703c33bf831a100',
  balance: '1000000000000000000',
};

export const validatorsResponse: ValidatorsZilliqaResponse = {
  items: [ validator1 ],
  next_page_params: null,
};

export const validatorDetails: ValidatorZilliqa = {
  added_at_block_number: 7527600,
  balance: '20000000000000000000000000',
  bls_public_key: '0x95125dca41be848801f9bd75254f1faf1ae3194b1da53e9a5684ed7f67b729542482bc521924603b9703c33bf831a100',
  control_address: {
    ens_domain_name: null,
    hash: '0xB4492C468Fe97CB73Ea70a9A712cdd5B5aB621c3',
    implementations: [],
    is_contract: false,
    is_verified: null,
    metadata: null,
    name: null,
    private_tags: [],
    proxy_type: null,
    public_tags: [],
    watchlist_names: [],
  },
  index: 1,
  peer_id: '0x002408011220a8ce8c9a146f3dc411cd72ba845b76722824c55824ac74b3362f070a332d85f2',
  reward_address: {
    ens_domain_name: null,
    hash: '0x0000000000000000000000000000000000000000',
    implementations: [],
    is_contract: false,
    is_verified: null,
    metadata: null,
    name: null,
    private_tags: [],
    proxy_type: null,
    public_tags: [],
    watchlist_names: [],
  },
  signing_address: {
    ens_domain_name: null,
    hash: '0x0000000000000000000000000000000000000026',
    implementations: [],
    is_contract: false,
    is_verified: null,
    metadata: null,
    name: null,
    private_tags: [],
    proxy_type: null,
    public_tags: [],
    watchlist_names: [],
  },
  stake_updated_at_block_number: 7527642,
};
