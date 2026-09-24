import type { TextareaProps as ChakraTextareaProps } from '@chakra-ui/react';
import { Textarea as ChakraTextarea } from '@chakra-ui/react';

export interface TextareaProps extends ChakraTextareaProps {}

export const Textarea = ChakraTextarea;
