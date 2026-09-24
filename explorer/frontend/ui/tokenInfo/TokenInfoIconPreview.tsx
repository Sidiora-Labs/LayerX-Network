import { Center } from '@chakra-ui/react';
import React from 'react';

interface Props {
  url: string | undefined;
  isInvalid: boolean;
  children: React.ReactElement;
}

const TokenInfoIconPreview = ({ url, isInvalid, children }: Props) => {
  const borderColorActive = isInvalid ? 'error' : 'input.border.filled';

  return (
    <Center
      boxSize="60px"
      flexShrink={ 0 }
      borderWidth="2px"
      borderColor={ url ? borderColorActive : 'input.border' }
      borderRadius="base"
    >
      { children }
    </Center>
  );
};

export default React.memo(TokenInfoIconPreview);
