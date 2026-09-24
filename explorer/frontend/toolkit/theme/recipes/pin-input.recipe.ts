import { defineSlotRecipe } from '@chakra-ui/react';

import { mapEntries } from '../utils/entries';
import { recipe as inputRecipe } from './input.recipe';

const { variants } = inputRecipe;

export const recipe = defineSlotRecipe({
  slots: [ 'input' ],
  base: {
    input: {
      ...inputRecipe.base,
      textAlign: 'center',
      width: 'var(--input-height)',
    },
  },
  variants: {
    size: {
      md: {
        input: {
          boxSize: 10,
          borderRadius: 'base',
        },
      },
    },
    variant: mapEntries(variants!.variant, (key, value) => [
      key,
      { input: value },
    ]),
  },
  defaultVariants: {
    size: 'md',
    variant: 'outline',
  },
});
