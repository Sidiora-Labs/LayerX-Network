import type { AdvancedFilterAge, AdvancedFilterParams } from 'types/api/advancedFilter';

import BaseAgeFilter, { type DateConverter } from './BaseAgeFilter';

const FILTER_PARAM_FROM = 'age_from';
const FILTER_PARAM_TO = 'age_to';
const FILTER_PARAM_AGE = 'age';

const dateConverter: DateConverter = {
  toFilterValue: (date: string) => date, // Keep as ISO string
  fromFilterValue: (value: string | undefined) => value || '',
  getCurrentValue: (value: string | undefined) => value || '',
};

type Props = {
  value?: { age: AdvancedFilterAge | ''; from: string; to: string };
  handleFilterChange: (field: keyof AdvancedFilterParams, value?: string) => void;
  columnName: string;
  isLoading?: boolean;
  onClose?: () => void;
};

const AgeFilter = (props: Props) => {
  return (
    <BaseAgeFilter<AdvancedFilterParams>
      { ...props }
      fieldNames={{
        from: FILTER_PARAM_FROM,
        to: FILTER_PARAM_TO,
        age: FILTER_PARAM_AGE,
      }}
      dateConverter={ dateConverter }
    />
  );
};

export default AgeFilter;
