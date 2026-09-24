import { Flex, Text } from '@chakra-ui/react';
import { isEqual } from 'es-toolkit';
import type { ChangeEvent } from 'react';
import React from 'react';

import type { AdvancedFilterParams } from 'types/api/advancedFilter';

import { Input } from 'toolkit/chakra/input';
import { PopoverCloseTriggerWrapper } from 'toolkit/chakra/popover';
import { ndash } from 'toolkit/utils/htmlEntities';
import TableColumnFilter from 'ui/shared/filters/TableColumnFilter';
import TagGroupSelect from 'ui/shared/tagGroupSelect/TagGroupSelect';

const FILTER_PARAM_FROM = 'amount_from';
const FILTER_PARAM_TO = 'amount_to';

const PRESETS = [
  {
    value: '10',
    name: '<10',
  },
  {
    value: '100',
    name: '<100',
  },
  {
    value: '1000',
    name: '<1K',
  },
  {
    value: '10000',
    name: '<10K',
  },
  {
    value: '100000',
    name: '<100K',
  },
  {
    value: '1000000',
    name: '<1M',
  },
];

const defaultValue = { from: '', to: '' };
type AmountValue = { from?: string; to?: string };

type Props = {
  value?: AmountValue;
  handleFilterChange: (filed: keyof AdvancedFilterParams, value?: string) => void;
};

const AmountFilter = ({ value = {}, handleFilterChange }: Props) => {
  const [ currentValue, setCurrentValue ] = React.useState<AmountValue>(value || defaultValue);

  const handleFromChange = React.useCallback((event: ChangeEvent<HTMLInputElement>) => {
    setCurrentValue(prev => ({ ...prev, from: event.target.value }));
  }, []);

  const handleToChange = React.useCallback((event: ChangeEvent<HTMLInputElement>) => {
    setCurrentValue(prev => ({ ...prev, to: event.target.value }));
  }, []);

  const onReset = React.useCallback(() => setCurrentValue(defaultValue), []);

  const onFilter = React.useCallback(() => {
    handleFilterChange(FILTER_PARAM_FROM, currentValue.from);
    handleFilterChange(FILTER_PARAM_TO, currentValue.to);
  }, [ handleFilterChange, currentValue ]);

  const onPresetChange = React.useCallback((to: string) => {
    setCurrentValue({ from: '', to });
    handleFilterChange(FILTER_PARAM_FROM, '');
    handleFilterChange(FILTER_PARAM_TO, to);
  }, [ handleFilterChange ]);

  return (
    <TableColumnFilter
      title="Amount"
      isFilled={ Boolean(currentValue.from || currentValue.to) }
      isTouched={ !isEqual(currentValue, value) }
      onFilter={ onFilter }
      onReset={ onReset }
      hasReset
    >
      <PopoverCloseTriggerWrapper>
        <Flex gap={ 3 }>
          <TagGroupSelect<string>
            items={ PRESETS.map((preset) => ({ id: preset.value, title: preset.name })) }
            onChange={ onPresetChange }
            value={ (!currentValue.from || currentValue.from === '0') ? PRESETS.find(preset => preset.value === currentValue.to)?.value : undefined }
          />
        </Flex>
      </PopoverCloseTriggerWrapper>
      <Flex mt={ 3 } alignItems="center">
        <Input value={ currentValue.from } onChange={ handleFromChange } placeholder="From" type="number" size="sm"/>
        <Text mx={ 3 }>{ ndash }</Text>
        <Input value={ currentValue.to } onChange={ handleToChange } placeholder="To" type="number" size="sm"/>
      </Flex>
    </TableColumnFilter>
  );
};

export default AmountFilter;
