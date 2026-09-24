import React from 'react';

import type { NFTTokenType } from 'types/api/token';

import PopoverFilter from 'ui/shared/filters/PopoverFilter';
import TokenTypeFilter from 'ui/shared/filters/TokenTypeFilter';

interface Props {
  value: Array<NFTTokenType> | undefined;
  onChange: (value: Array<NFTTokenType>) => void;
}

const AddressNftTypeFilter = ({ value, onChange }: Props) => {
  return (
    <PopoverFilter contentProps={{ w: '200px' }} appliedFiltersNum={ value?.length }>
      <TokenTypeFilter<NFTTokenType> nftOnly onChange={ onChange } defaultValue={ value }/>
    </PopoverFilter>
  );
};

export default React.memo(AddressNftTypeFilter);
