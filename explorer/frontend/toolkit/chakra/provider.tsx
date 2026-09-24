'use client';

import { ChakraProvider } from '@chakra-ui/react';
import React from 'react';

import theme from '../theme/theme';
import {
  ColorModeProvider,
  type ColorModeProviderProps,
} from './color-mode';

export function Provider(props: ColorModeProviderProps) {
  return (
    <ChakraProvider value={ theme }>
      <ColorModeProvider { ...props }/>
    </ChakraProvider>
  );
}
