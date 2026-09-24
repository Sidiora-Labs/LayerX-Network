import type { HTMLChakraProps } from '@chakra-ui/react';
import { chakra } from '@chakra-ui/react';
import React from 'react';

const TextSeparator = ({ id, ...props }: HTMLChakraProps<'span'> & { id?: string }) => {
  return <chakra.span mx={{ base: 2, lg: 3 }} id={ id } { ...props } color="border.divider">|</chakra.span>;
};

export default React.memo(TextSeparator);
