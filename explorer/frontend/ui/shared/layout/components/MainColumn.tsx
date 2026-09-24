import { Flex, chakra } from '@chakra-ui/react';
import React from 'react';

import config from 'configs/app';

interface Props {
  className?: string;
  children: React.ReactNode;
}

const MainColumn = ({ children, className }: Props) => {
  return (
    <Flex
      className={ className }
      flexDir="column"
      flexGrow={ 1 }
      w={{ base: '100%', lg: config.UI.navigation.layout === 'horizontal' ? '100%' : 'auto' }}
      paddingX={{ base: 3, lg: config.UI.navigation.layout === 'horizontal' ? 6 : 12 }}
      paddingRight={{ '2xl': 6 }}
      paddingTop={{ base: '12px', lg: 6 }} // 12px is top padding of content area
      paddingBottom={ 8 }
    >
      { children }
    </Flex>
  );
};

export default React.memo(chakra(MainColumn));
