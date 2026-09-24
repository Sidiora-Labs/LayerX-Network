import { Flex } from '@chakra-ui/react';
import React from 'react';

import type { ZetaChainCCTXFilterParams, StatusReducedFilters } from 'types/client/zetaChain';

import { Radio, RadioGroup } from 'toolkit/chakra/radio';
import TableColumnFilter from 'ui/shared/filters/TableColumnFilter';

const FILTER_PARAM_STATUS = 'status_reduced';

type FilterValue = 'all' | StatusReducedFilters;

const STATUS_OPTIONS: Array<{ value: FilterValue; label: string }> = [
  { value: 'all', label: 'All' },
  { value: 'Success', label: 'Success' },
  { value: 'Failed', label: 'Failed' },
];

type Props = {
  value?: Array<StatusReducedFilters>;
  handleFilterChange: (field: keyof ZetaChainCCTXFilterParams, value?: Array<StatusReducedFilters>) => void;
  columnName: string;
  isLoading?: boolean;
  onClose?: () => void;
};

const ZetaChainStatusFilter = ({ value = [], handleFilterChange, onClose }: Props) => {
  // Convert API values to internal format and determine initial state
  const getInitialValue = React.useCallback((): FilterValue => {
    if (value.length === 1) {
      return value[0];
    }
    return 'all';
  }, [ value ]);

  const [ currentValue, setCurrentValue ] = React.useState<FilterValue>(getInitialValue());

  const handleStatusChange = React.useCallback((value: FilterValue) => {
    setCurrentValue(value);
  }, []);

  const onReset = React.useCallback(() => setCurrentValue('all'), []);

  const onFilter = React.useCallback(() => {
    let value: Array<StatusReducedFilters> | undefined;

    if (currentValue === 'all') {
      value = undefined;
    } else {
      value = [ currentValue ];
    }

    handleFilterChange(FILTER_PARAM_STATUS, value);
    onClose?.();
  }, [ handleFilterChange, currentValue, onClose ]);

  // Check if the current value is different from the initial value
  const isTouched = React.useMemo(() => {
    const initialValue = getInitialValue();
    return currentValue !== initialValue;
  }, [ currentValue, getInitialValue ]);

  const handleRadioChange = React.useCallback(({ value }: { value: string | null }) => {
    if (value) {
      handleStatusChange(value as FilterValue);
    }
  }, [ handleStatusChange ]);

  return (
    <TableColumnFilter
      title="Status"
      isFilled={ currentValue !== 'all' }
      isTouched={ isTouched }
      onFilter={ onFilter }
      onReset={ onReset }
      hasReset
    >
      <Flex direction="column" gap={ 2 }>
        <RadioGroup value={ currentValue } onValueChange={ handleRadioChange }>
          { STATUS_OPTIONS.map(option => (
            <Radio key={ option.value } value={ option.value }>
              { option.label }
            </Radio>
          )) }
        </RadioGroup>
      </Flex>
    </TableColumnFilter>
  );
};

export default ZetaChainStatusFilter;
