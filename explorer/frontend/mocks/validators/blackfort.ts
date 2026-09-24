import type {
  ValidatorBlackfort,
  ValidatorsBlackfortCountersResponse,
  ValidatorsBlackfortResponse,
} from 'types/api/validators';

import * as addressMock from '../address/address';

export const validator1: ValidatorBlackfort = {
  address: addressMock.withName,
  name: 'testnet-3',
  commission: 10,
  delegated_amount: '0',
  self_bonded_amount: '10000',
};

export const validator2: ValidatorBlackfort = {
  address: addressMock.withEns,
  name: 'GooseGanG GooseGanG GooseGanG GooseGanG GooseGanG GooseGanG GooseGanG',
  commission: 5000,
  delegated_amount: '10000',
  self_bonded_amount: '100',
};

export const validator3: ValidatorBlackfort = {
  address: addressMock.withoutName,
  name: 'testnet-1',
  commission: 0,
  delegated_amount: '0',
  self_bonded_amount: '10000',
};

export const validatorsResponse: ValidatorsBlackfortResponse = {
  items: [ validator1, validator2, validator3 ],
  next_page_params: null,
};

export const validatorsCountersResponse: ValidatorsBlackfortCountersResponse = {
  new_validators_count_24h: '11',
  validators_count: '140',
};
