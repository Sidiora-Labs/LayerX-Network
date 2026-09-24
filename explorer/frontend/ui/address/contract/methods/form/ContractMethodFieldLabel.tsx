import { Box } from '@chakra-ui/react';
import React from 'react';

import type { ContractAbiItemInput } from '../types';

import { Hint } from 'toolkit/components/Hint/Hint';

import { getFieldLabel } from './utils';

interface Props {
  data: ContractAbiItemInput;
  isOptional?: boolean;
  level: number;
}

const ContractMethodFieldLabel = ({ data, isOptional, level }: Props) => {
  return (
    <Box
      w="250px"
      textStyle="sm"
      py="6px"
      flexShrink={ 0 }
      fontWeight={ 500 }
      color={ level > 1 ? { _light: 'blackAlpha.600', _dark: 'whiteAlpha.600' } : undefined }
      wordBreak="break-all"
    >
      { getFieldLabel(data, !isOptional) }
      { data.type === 'string' && <Hint label={ `The "" string will be treated as an empty string` } ml={ 2 }/> }
    </Box>
  );
};

export default React.memo(ContractMethodFieldLabel);
