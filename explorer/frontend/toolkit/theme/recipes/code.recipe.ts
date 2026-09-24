import { defineRecipe } from '@chakra-ui/react';

import { recipe as badgeRecipe } from './badge.recipe';

const { variants, defaultVariants } = badgeRecipe;

export const recipe = defineRecipe({
  className: 'chakra-code',
  base: {
    fontFamily: 'mono',
    alignItems: 'center',
    display: 'inline-flex',
    borderRadius: 'l2',
  },
  variants,
  defaultVariants: {
    ...defaultVariants,
    size: 'sm',
  },
});
