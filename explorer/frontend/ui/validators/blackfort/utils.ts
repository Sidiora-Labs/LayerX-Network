import type {
  ValidatorsBlackfortSortingValue,
  ValidatorsBlackfortSortingField,
} from 'types/api/validators';

import type { SelectOption } from 'toolkit/chakra/select';

export const VALIDATORS_BLACKFORT_SORT_OPTIONS: Array<SelectOption<ValidatorsBlackfortSortingValue>> = [
  { label: 'Default', value: 'default' },
  { label: 'Address descending', value: 'address_hash-desc' },
  { label: 'Address ascending', value: 'address_hash-asc' },
];

export const VALIDATORS_BLACKFORT_SORT_SEQUENCE: Record<ValidatorsBlackfortSortingField, Array<ValidatorsBlackfortSortingValue>> = {
  address_hash: [ 'address_hash-desc', 'address_hash-asc', 'default' ],
};
