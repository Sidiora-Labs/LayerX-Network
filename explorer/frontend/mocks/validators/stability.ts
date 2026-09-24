import type {
  ValidatorStability,
  ValidatorsStabilityCountersResponse,
  ValidatorsStabilityResponse,
} from 'types/api/validators';

import * as addressMock from '../address/address';

export const validator1: ValidatorStability = {
  address: addressMock.withName,
  blocks_validated_count: 7334224,
  state: 'active',
};

export const validator2: ValidatorStability = {
  address: addressMock.withEns,
  blocks_validated_count: 8937453,
  state: 'probation',
};

export const validator3: ValidatorStability = {
  address: addressMock.withoutName,
  blocks_validated_count: 1234,
  state: 'inactive',
};

export const validatorsResponse: ValidatorsStabilityResponse = {
  items: [ validator1, validator2, validator3 ],
  next_page_params: null,
};

export const validatorsCountersResponse: ValidatorsStabilityCountersResponse = {
  active_validators_count: '42',
  active_validators_percentage: 7.14,
  new_validators_count_24h: '11',
  validators_count: '140',
};
