import type {
  ValidatorsStabilitySortingValue,
  ValidatorsStabilitySortingField,
} from 'types/api/validators';

import type { SelectOption } from 'toolkit/chakra/select';

export const VALIDATORS_STABILITY_SORT_OPTIONS: Array<SelectOption<ValidatorsStabilitySortingValue>> = [
  { label: 'Default', value: 'default' },
  { label: 'Status descending', value: 'state-desc' },
  { label: 'Status ascending', value: 'state-asc' },
  { label: 'Blocks validated descending', value: 'blocks_validated-desc' },
  { label: 'Blocks validated ascending', value: 'blocks_validated-asc' },
];

export const VALIDATORS_STABILITY_SORT_SEQUENCE: Record<ValidatorsStabilitySortingField, Array<ValidatorsStabilitySortingValue>> = {
  state: [ 'state-desc', 'state-asc', 'default' ],
  blocks_validated: [ 'blocks_validated-desc', 'blocks_validated-asc', 'default' ],
};
