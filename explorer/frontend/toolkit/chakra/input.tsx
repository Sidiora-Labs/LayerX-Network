import type { InputProps as ChakraInputProps } from '@chakra-ui/react';
import { Input as ChakraInput } from '@chakra-ui/react';

export interface InputProps extends Omit<ChakraInputProps, 'size'> {
  size?: 'sm' | 'md' | 'lg' | '2xl';
}

export const Input = ChakraInput;
