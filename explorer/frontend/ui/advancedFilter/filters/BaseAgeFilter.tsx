import { Flex, Text } from '@chakra-ui/react';
import { isEqual } from 'es-toolkit';
import type { ChangeEvent } from 'react';
import React from 'react';

import { ADVANCED_FILTER_AGES, type AdvancedFilterAge } from 'types/api/advancedFilter';

import dayjs from 'lib/date/dayjs';
import { Input } from 'toolkit/chakra/input';
import { PopoverCloseTriggerWrapper } from 'toolkit/chakra/popover';
import { ndash } from 'toolkit/utils/htmlEntities';
import TableColumnFilter from 'ui/shared/filters/TableColumnFilter';
import TagGroupSelect from 'ui/shared/tagGroupSelect/TagGroupSelect';

import { getDurationFromAge } from '../lib';

const defaultValue = { age: '', from: '', to: '' } as const;
type AgeFromToValue = { age: AdvancedFilterAge | ''; from: string; to: string };

type DateConverter = {
  toFilterValue: (date: string) => string;
  fromFilterValue: (value: string | undefined) => string;
  getCurrentValue: (value: string | undefined) => string;
};

type Props<T> = {
  value?: AgeFromToValue;
  handleFilterChange: (field: keyof T, value?: string) => void;
  columnName: string;
  isLoading?: boolean;
  onClose?: () => void;
  fieldNames: {
    from: keyof T;
    to: keyof T;
    age: keyof T;
  };
  dateConverter: DateConverter;
};

const DateInput = ({ value, onChange, placeholder, max }: { value: string; onChange: (value: string) => void; placeholder: string; max: string }) => {
  const [ tempValue, setTempValue ] = React.useState(value || '');

  // reset
  React.useEffect(() => {
    if (!value) {
      setTempValue('');
    }
  }, [ value ]);

  const handleChange = React.useCallback((event: ChangeEvent<HTMLInputElement>) => {
    setTempValue(event.target.value);
    onChange(event.target.value);
  }, [ onChange ]);

  return (
    <Input
      value={ tempValue }
      onChange={ handleChange }
      placeholder={ placeholder }
      type="date"
      max={ max }
      autoComplete="off"
      size="sm"
    />
  );
};

function BaseAgeFilter<T>({
  value = defaultValue,
  handleFilterChange,
  onClose,
  fieldNames,
  dateConverter,
}: Props<T>) {
  const getFromValue = () => {
    if (value.age) return '';
    return value.from ? dateConverter.getCurrentValue(value.from) : '';
  };

  const getToValue = () => {
    if (value.age) return '';
    return value.to ? dateConverter.getCurrentValue(value.to) : '';
  };

  const [ currentValue, setCurrentValue ] = React.useState<AgeFromToValue>({
    age: value.age || '',
    from: getFromValue(),
    to: getToValue(),
  });

  const handleFromChange = React.useCallback((newValue: string) => {
    setCurrentValue(prev => ({ age: '', to: prev.to, from: newValue }));
  }, []);

  const handleToChange = React.useCallback((newValue: string) => {
    setCurrentValue(prev => ({ age: '', from: prev.from, to: newValue }));
  }, []);

  const onPresetChange = React.useCallback((age: AdvancedFilterAge) => {
    const from = dateConverter.toFilterValue(dayjs((dayjs().valueOf() - getDurationFromAge(age))).toISOString());
    handleFilterChange(fieldNames.from, from);
    const to = dateConverter.toFilterValue(dayjs().toISOString());
    handleFilterChange(fieldNames.to, to);
    handleFilterChange(fieldNames.age, age);
    onClose?.();
  }, [ handleFilterChange, onClose, fieldNames, dateConverter ]);

  const onReset = React.useCallback(() => {
    setCurrentValue(defaultValue);
  }, []);

  const onFilter = React.useCallback(() => {
    if (!currentValue.age && !currentValue.to && !currentValue.from) {
      handleFilterChange(fieldNames.from, undefined);
      handleFilterChange(fieldNames.to, undefined);
      handleFilterChange(fieldNames.age, undefined);
      return;
    }

    if (currentValue.age) {
      // Age preset is selected, calculate timestamps
      const from = dateConverter.toFilterValue(dayjs((dayjs().valueOf() - getDurationFromAge(currentValue.age))).toISOString());
      const to = dateConverter.toFilterValue(dayjs().toISOString());
      handleFilterChange(fieldNames.from, from);
      handleFilterChange(fieldNames.to, to);
      handleFilterChange(fieldNames.age, currentValue.age);
    } else {
      // Custom date range is selected
      const from = currentValue.from ? dateConverter.toFilterValue(dayjs(currentValue.from).startOf('day').toISOString()) : undefined;
      const to = currentValue.to ? dateConverter.toFilterValue(dayjs(currentValue.to).endOf('day').toISOString()) : undefined;
      handleFilterChange(fieldNames.from, from);
      handleFilterChange(fieldNames.to, to);
      handleFilterChange(fieldNames.age, undefined);
    }
  }, [ handleFilterChange, currentValue, fieldNames, dateConverter ]);

  // Check if the current values differ from the original values
  const isTouched = React.useMemo(() => {
    if (currentValue.age) {
      return value.age !== currentValue.age;
    }

    // If both current values are empty and both original values are empty, not touched
    if (!currentValue.from && !currentValue.to && !value.from && !value.to) {
      return false;
    }

    // Convert original values to date strings for comparison
    const originalValueAsDates = {
      from: value.from ? dateConverter.getCurrentValue(value.from) : '',
      to: value.to ? dateConverter.getCurrentValue(value.to) : '',
    };

    // Compare the date strings directly
    return !isEqual(currentValue, originalValueAsDates);
  }, [ currentValue, value, dateConverter ]);

  return (
    <TableColumnFilter
      title="Set last duration"
      isFilled={ Boolean(currentValue.from || currentValue.to || currentValue.age) }
      isTouched={ isTouched }
      onFilter={ onFilter }
      onReset={ onReset }
      hasReset
    >
      <Flex gap={ 3 }>
        <PopoverCloseTriggerWrapper>
          <TagGroupSelect<AdvancedFilterAge>
            items={ ADVANCED_FILTER_AGES.map(val => ({ id: val, title: val })) }
            onChange={ onPresetChange }
            value={ currentValue.age || undefined }
          />
        </PopoverCloseTriggerWrapper>
      </Flex>
      <Flex mt={ 3 }>
        <DateInput
          value={ currentValue.age ? '' : currentValue.from }
          onChange={ handleFromChange }
          placeholder="From"
          max={ dayjs().format('YYYY-MM-DD') }
        />
        <Text mx={ 3 }>{ ndash }</Text>
        <DateInput
          value={ currentValue.age ? '' : currentValue.to }
          onChange={ handleToChange }
          placeholder="To"
          max={ dayjs().format('YYYY-MM-DD') }
        />
      </Flex>
    </TableColumnFilter>
  );
}

export default BaseAgeFilter;
export type { Props, DateConverter };
