import type { HTMLChakraProps } from '@chakra-ui/react';
import React from 'react';

import IconSvg from 'ui/shared/IconSvg';

interface Props extends HTMLChakraProps<'div'> {
  isFavorite: boolean;
};

const FavoriteIcon = ({ isFavorite, ...rest }: Props) => {
  return (
    <IconSvg
      name={ isFavorite ? 'heart_filled' : 'heart_outline' }
      { ...rest }
    />
  );
};

export default FavoriteIcon;
