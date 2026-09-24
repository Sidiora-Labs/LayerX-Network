import type { AdvancedFilterAge } from 'types/api/advancedFilter';
import type { ZetaChainCCTXFilterParams } from 'types/client/zetaChain';

import dayjs from 'lib/date/dayjs';
import { SECOND } from 'toolkit/utils/consts';
import BaseAgeFilter, { type DateConverter } from 'ui/advancedFilter/filters/BaseAgeFilter';

const FILTER_PARAM_FROM = 'start_timestamp';
const FILTER_PARAM_TO = 'end_timestamp';
const FILTER_PARAM_AGE = 'age';

const dateConverter: DateConverter = {
  toFilterValue: (date: string) => dayjs(date).unix().toString(), // Convert ISO to Unix timestamp
  fromFilterValue: (value: string | undefined) => value || '',
  getCurrentValue: (value: string | undefined) => value ? dayjs(Number(value) * SECOND).format('YYYY-MM-DD') : '',
};

type Props = {
  value?: { age: AdvancedFilterAge | ''; from: string; to: string };
  handleFilterChange: (field: keyof ZetaChainCCTXFilterParams, value?: string) => void;
  columnName: string;
  isLoading?: boolean;
  onClose?: () => void;
};

const ZetaChainAgeFilter = (props: Props) => {
  return (
    <BaseAgeFilter<ZetaChainCCTXFilterParams>
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

export default ZetaChainAgeFilter;
