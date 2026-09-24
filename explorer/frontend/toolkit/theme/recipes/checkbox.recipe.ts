import { defineSlotRecipe } from '@chakra-ui/react';

import { recipe as checkmarkRecipe } from './checkmark.recipe';

export const recipe = defineSlotRecipe({
  slots: [ 'root', 'control', 'label' ],
  className: 'chakra-checkbox',
  base: {
    root: {
      display: 'inline-flex',
      gap: '2',
      alignItems: 'center',
      verticalAlign: 'top',
      position: 'relative',
      cursor: 'pointer',
      _disabled: {
        cursor: 'disabled',
      },
      _readOnly: {
        cursor: 'default',
      },
    },

    control: checkmarkRecipe.base,

    label: {
      fontWeight: 'normal',
      userSelect: 'none',
      flexGrow: 1,
      _disabled: {
        opacity: 'control.disabled',
      },
    },
  },

  variants: {
    size: {
      md: {
        root: { gap: '2' },
        label: { textStyle: 'md' },
        control: checkmarkRecipe.variants?.size?.md,
      },
    },

    variant: {
      solid: {
        control: checkmarkRecipe.variants?.variant?.solid,
      },
    },
  },

  defaultVariants: {
    variant: 'solid',
    size: 'md',
  },
});
