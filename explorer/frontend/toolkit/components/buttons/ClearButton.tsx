import React from 'react';

import type { CloseButtonProps } from '../../chakra/close-button';
import { CloseButton } from '../../chakra/close-button';

export interface ClearButtonProps extends CloseButtonProps {
  visible?: boolean;
}

export const ClearButton = ({ disabled, visible = true, ...rest }: ClearButtonProps) => {
  return (
    <CloseButton
      disabled={ disabled || !visible }
      aria-label="Clear"
      title="Clear"
      opacity={ visible ? 1 : 0 }
      visibility={ visible ? 'visible' : 'hidden' }
      color={{ _light: 'gray.300', _dark: 'whiteAlpha.300' }}
      { ...rest }
    />
  );
};
