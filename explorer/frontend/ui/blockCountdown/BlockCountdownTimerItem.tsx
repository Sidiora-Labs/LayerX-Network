import { Box } from '@chakra-ui/react';
import React from 'react';

import { TruncatedText } from 'toolkit/components/truncation/TruncatedText';

interface Props {
  label: string;
  value: string;
}

const BlockCountdownTimerItem = ({ label, value }: Props) => {
  return (
    <Box
      minW={{ base: '70px', lg: '100px' }}
      textAlign="center"
      overflow="hidden"
      flex="1 1 auto"
    >
      <TruncatedText
        text={ value }
        fontFamily="heading"
        fontSize={{ base: '40px', lg: '48px' }}
        lineHeight="48px"
        fontWeight={ 600 }
        w="100%"
      />
      <Box fontSize="sm" lineHeight="20px" mt={ 1 } color="text.secondary">{ label }</Box>
    </Box>
  );
};

export default React.memo(BlockCountdownTimerItem);
