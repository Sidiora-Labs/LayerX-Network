import { defineSlotRecipe } from '@chakra-ui/react';

export const recipe = defineSlotRecipe({
  className: 'chakra-rating-group',
  slots: [ 'root', 'control', 'item', 'itemIndicator' ],
  base: {
    root: {
      display: 'inline-flex',
      alignItems: 'center',
      columnGap: 3,
    },

    control: {
      display: 'inline-flex',
      alignItems: 'center',
      gap: 1,
    },

    item: {
      display: 'inline-flex',
      alignItems: 'center',
      justifyContent: 'center',
      userSelect: 'none',
      cursor: 'pointer',

      _icon: {
        width: '100%',
        height: '100%',
        display: 'inline-block',
        flexShrink: 0,
        position: 'absolute',
        left: 0,
        top: 0,
      },
    },

    itemIndicator: {
      display: 'inline-flex',
      alignItems: 'center',
      justifyContent: 'center',
      position: 'relative',

      _icon: {
        stroke: 'none',
        width: '100%',
        height: '100%',
        display: 'inline-block',
        flexShrink: 0,
        position: 'absolute',
        left: 0,
        top: 0,
      },

      '& [data-bg]': {
        color: 'rating',
      },

      '& [data-fg]': {
        color: 'transparent',
      },

      '&[data-highlighted]:not([data-half])': {
        '& [data-fg]': {
          color: 'rating.highlighted',
        },
      },

      '&[data-half]': {
        '& [data-fg]': {
          color: 'rating.highlighted',
          clipPath: 'inset(0 50% 0 0)',
        },
      },
    },
  },

  variants: {
    size: {
      md: {
        item: {
          boxSize: 5,
        },
        root: {
          textStyle: 'md',
        },
      },
    },
  },

  defaultVariants: {
    size: 'md',
  },
});
