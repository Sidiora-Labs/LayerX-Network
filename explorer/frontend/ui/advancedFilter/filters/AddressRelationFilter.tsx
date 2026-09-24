import { Box } from '@chakra-ui/react';
import React from 'react';

import { type AdvancedFilterParams } from 'types/api/advancedFilter';

import { Radio, RadioGroup } from 'toolkit/chakra/radio';

const FILTER_PARAM = 'address_relation';

type Value = 'or' | 'and';

const DEFAULT_VALUE = 'or' as Value;

type Props = {
  value?: Value;
  handleFilterChange: (filed: keyof AdvancedFilterParams, value?: string) => void;
  columnName: string;
  isLoading?: boolean;
  onClose?: () => void;
};

const AddressRelationFilter = ({ value = DEFAULT_VALUE, handleFilterChange, onClose }: Props) => {
  const onFilter = React.useCallback(({ value }: { value: string | null }) => {
    if (!value) {
      return;
    }

    onClose && onClose();
    handleFilterChange(FILTER_PARAM, value as Value);
  }, [ handleFilterChange, onClose ]);

  return (
    <Box w="120px">
      <RadioGroup onValueChange={ onFilter } value={ value } orientation="vertical">
        <Radio value="or">OR</Radio>
        <Radio value="and">AND</Radio>
      </RadioGroup>
    </Box>
  );
};

export default AddressRelationFilter;
