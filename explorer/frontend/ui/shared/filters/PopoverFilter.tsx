import React from 'react';

import type { PopoverContentProps } from 'toolkit/chakra/popover';
import { PopoverBody, PopoverContent, PopoverTrigger, PopoverRoot } from 'toolkit/chakra/popover';
import FilterButton from 'ui/shared/filters/FilterButton';

interface Props {
  appliedFiltersNum?: number;
  children: React.ReactNode;
  contentProps?: PopoverContentProps;
  isLoading?: boolean;
}

const PopoverFilter = ({ appliedFiltersNum, children, contentProps, isLoading }: Props) => {
  return (
    <PopoverRoot>
      <PopoverTrigger className="group">
        <FilterButton
          appliedFiltersNum={ appliedFiltersNum }
          isLoading={ isLoading }
        />
      </PopoverTrigger>
      <PopoverContent { ...contentProps }>
        <PopoverBody display="flex" flexDir="column" rowGap={ 5 }>
          { children }
        </PopoverBody>
      </PopoverContent>
    </PopoverRoot>
  );
};

export default React.memo(PopoverFilter);
