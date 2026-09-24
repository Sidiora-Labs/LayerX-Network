import { defineSlotRecipe } from '@chakra-ui/react';

import { recipe as radiomarkRecipe } from './radiomark.recipe';

export const recipe = defineSlotRecipe({
  slots: [ 'item', 'itemControl', 'itemText', 'root' ],
  base: {
    root: {
      display: 'flex',
    },

    item: {
      display: 'inline-flex',
      alignItems: 'center',
      position: 'relative',
      fontWeight: 'normal',
      cursor: 'pointer',
      _disabled: {
        cursor: 'disabled',
      },
      _readOnly: {
        cursor: 'default',
      },
    },

    itemControl: radiomarkRecipe.base,

    itemText: {
      userSelect: 'none',
      textStyle: 'md',
      _disabled: {
        opacity: 'control.disabled',
      },
    },
  },
  variants: {
    variant: {
      solid: {
        itemControl: radiomarkRecipe.variants?.variant?.solid,
      },
    },

    size: {
      md: {
        item: { textStyle: 'md', gap: '2' },
        itemControl: radiomarkRecipe.variants?.size?.md,
      },
    },

    orientation: {
      vertical: {
        root: {
          flexDirection: 'column',
          alignItems: 'flex-start',
          rowGap: '12px',
        },
      },
      horizontal: {
        root: {
          flexDirection: 'row',
          alignItems: 'center',
          columnGap: '32px',
        },
      },
    },
  },

  defaultVariants: {
    size: 'md',
    variant: 'solid',
    orientation: 'vertical',
  },
});
