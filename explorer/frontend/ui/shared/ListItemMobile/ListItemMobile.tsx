import { Flex, chakra } from '@chakra-ui/react';
import React from 'react';

interface Props {
  children: React.ReactNode;
  className?: string;
  animation?: string;
}

const ListItemMobile = ({ children, className, animation }: Props) => {
  return (
    <Flex
      animation={ animation }
      rowGap={ 6 }
      alignItems="flex-start"
      flexDirection="column"
      paddingY={ 6 }
      borderColor="border.divider"
      borderTopWidth="1px"
      _last={{
        borderBottomWidth: '1px',
      }}
      className={ className }
      fontSize="16px"
      lineHeight="20px"
    >
      { children }
    </Flex>
  );
};

export default chakra(ListItemMobile);
