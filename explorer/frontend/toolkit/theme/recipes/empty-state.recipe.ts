import { defineSlotRecipe } from '@chakra-ui/react';

export const recipe = defineSlotRecipe({
  slots: [ 'root', 'content', 'indicator', 'title', 'description' ],
  className: 'chakra-empty-state',
  base: {
    root: {
      width: 'full',
    },
    content: {
      display: 'flex',
      flexDirection: 'column',
      alignItems: 'center',
      justifyContent: 'center',
    },
    indicator: {
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      color: 'icon.secondary',
    },
    title: {
      fontWeight: 'semibold',
    },
    description: {
      color: 'text.secondary',
    },
  },

  variants: {
    size: {
      md: {
        root: {
          p: '0',
          mt: '50px',
        },
        title: {
          textStyle: 'heading.md',
        },
        content: {
          gap: { base: '4', lg: '6' },
        },
        description: {
          textStyle: { base: 'sm', lg: 'md' },
        },
        indicator: {
          _icon: {
            width: { base: '160px', lg: '260px' },
            height: 'auto',
          },
        },
      },
    },
  },

  defaultVariants: {
    size: 'md',
  },
});
