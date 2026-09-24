import type {
  ValidatorStability,
  ValidatorsStabilityCountersResponse,
  ValidatorBlackfort,
  ValidatorsBlackfortCountersResponse,
  ValidatorsZilliqaItem,
  ValidatorZilliqa,
} from 'types/api/validators';

import { ADDRESS_PARAMS } from './addressParams';

export const VALIDATOR_STABILITY: ValidatorStability = {
  address: ADDRESS_PARAMS,
  blocks_validated_count: 25987,
  state: 'active',
};

export const VALIDATORS_STABILITY_COUNTERS: ValidatorsStabilityCountersResponse = {
  active_validators_count: '42',
  active_validators_percentage: 7.14,
  new_validators_count_24h: '11',
  validators_count: '140',
};

export const VALIDATOR_BLACKFORT: ValidatorBlackfort = {
  address: ADDRESS_PARAMS,
  name: 'testnet-1',
  commission: 10,
  delegated_amount: '0',
  self_bonded_amount: '10000',
};

export const VALIDATORS_BLACKFORT_COUNTERS: ValidatorsBlackfortCountersResponse = {
  new_validators_count_24h: '11',
  validators_count: '140',
};

export const VALIDATORS_ZILLIQA_ITEM: ValidatorsZilliqaItem = {
  index: 420,
  bls_public_key: '0x95125dca41be848801f9bd75254f1faf1ae3194b1da53e9a5684ed7f67b729542482bc521924603b9703c33bf831a100',
  balance: '80000000000000000000000000',
};

export const VALIDATOR_ZILLIQA: ValidatorZilliqa = {
  index: 420,
  bls_public_key: '0x95125dca41be848801f9bd75254f1faf1ae3194b1da53e9a5684ed7f67b729542482bc521924603b9703c33bf831a100',
  balance: '1000000000000000000',
  added_at_block_number: 1234567890,
  control_address: ADDRESS_PARAMS,
  peer_id: '1234567890',
  reward_address: ADDRESS_PARAMS,
  signing_address: ADDRESS_PARAMS,
  stake_updated_at_block_number: 1234567890,
};
