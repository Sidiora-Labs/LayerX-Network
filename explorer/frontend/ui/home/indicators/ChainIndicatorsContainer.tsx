import { Flex } from '@chakra-ui/react';
import React from 'react';

interface Props {
  children: React.ReactNode;
}

const ChainIndicatorsContainer = ({ children }: Props) => {
  return (
    <Flex
      px={{ base: 3, lg: 4 }}
      py={ 3 }
      borderRadius="base"
      bgColor={{ _light: 'theme.stats.bg._light', _dark: 'theme.stats.bg._dark' }}
      columnGap={{ base: 3, lg: 4 }}
      rowGap={ 0 }
      flexBasis="50%"
      flexGrow={ 1 }
      alignItems="stretch"
    >
      { children }
    </Flex>
  );
};

export default React.memo(ChainIndicatorsContainer);
