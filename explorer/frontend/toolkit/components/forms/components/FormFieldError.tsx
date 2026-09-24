import { Box, chakra } from '@chakra-ui/react';
import React from 'react';

interface Props {
  message: string;
  className?: string;
}

export const FormFieldError = chakra(({ message, className }: Props) => {
  return <Box className={ className } color="text.error" textStyle="sm" mt={ 2 } wordBreak="break-word">{ message }</Box>;
});
