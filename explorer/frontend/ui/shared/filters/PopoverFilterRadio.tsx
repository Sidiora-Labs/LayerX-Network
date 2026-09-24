import type { ListCollection } from '@chakra-ui/react';
import React from 'react';

import type { SelectOption } from 'toolkit/chakra/select';
import { SelectContent, SelectItem, SelectRoot, SelectControl } from 'toolkit/chakra/select';
import FilterButton from 'ui/shared/filters/FilterButton';

interface Props {
  name: string;
  collection: ListCollection<SelectOption>;
  hasActiveFilter: boolean;
  initialValue?: string;
  isLoading?: boolean;
  onChange: (nextValue: string) => void;
}

const PopoverFilterRadio = ({ name, hasActiveFilter, collection, isLoading, onChange, initialValue }: Props) => {

  const handleValueChange = React.useCallback(({ value }: { value: Array<string> }) => {
    onChange(value[0]);
  }, [ onChange ]);

  return (
    <SelectRoot
      name={ name }
      collection={ collection }
      defaultValue={ initialValue ? [ initialValue ] : [ collection.items[0].value ] }
      onValueChange={ handleValueChange }
      w="fit-content"
      variant="plain"
    >
      <SelectControl
        triggerProps={{ asChild: true, px: { base: 1, lg: 2 } }}
        noIndicator
        defaultValue={ [ collection.items[0].value ] }
      >
        <FilterButton
          appliedFiltersNum={ hasActiveFilter ? 1 : 0 }
          isLoading={ isLoading }
        />
      </SelectControl>
      <SelectContent>
        { collection.items.map((item) => (
          <SelectItem item={ item } key={ item.value }>
            { item.label }
          </SelectItem>
        )) }
      </SelectContent>
    </SelectRoot>
  );
};

export default React.memo(PopoverFilterRadio);
