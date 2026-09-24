import React from 'react';

import type { FormattedData } from './types';

import { PopoverRoot, PopoverTrigger, PopoverContent, PopoverBody } from 'toolkit/chakra/popover';
import { useDisclosure } from 'toolkit/hooks/useDisclosure';

import TokenSelectButton from './TokenSelectButton';
import TokenSelectMenu from './TokenSelectMenu';
import useTokenSelect from './useTokenSelect';

interface Props {
  data: FormattedData;
  isLoading?: boolean;
}

const TokenSelectDesktop = ({ data, isLoading }: Props) => {
  const { open, onOpenChange } = useDisclosure();

  const result = useTokenSelect(data);

  return (
    <PopoverRoot open={ open } onOpenChange={ onOpenChange }>
      <PopoverTrigger>
        <TokenSelectButton data={ result.data } isLoading={ isLoading } isOpen={ open }/>
      </PopoverTrigger>
      <PopoverContent w="355px" maxH="450px" overflowY="scroll">
        <PopoverBody>
          <TokenSelectMenu { ...result }/>
        </PopoverBody>
      </PopoverContent>
    </PopoverRoot>
  );
};

export default React.memo(TokenSelectDesktop);
