import { defineSlotRecipe } from '@chakra-ui/react';

export const recipe = defineSlotRecipe({
  slots: [ 'root', 'title', 'description', 'indicator', 'content' ],

  base: {
    root: {
      width: 'full',
      display: 'flex',
      alignItems: 'flex-start',
      position: 'relative',
      borderRadius: 'base',
      color: 'alert.fg',
    },
    title: {
      fontWeight: '600',
    },
    description: {
      display: 'inline',
    },
    indicator: {
      display: 'inline-flex',
      alignItems: 'center',
      justifyContent: 'center',
      flexShrink: '0',
      width: '5',
      height: '5',
      _icon: { boxSize: 'full' },
      color: 'alert.fg',
    },
    content: {
      display: 'flex',
      flex: '1',
    },
  },

  variants: {
    status: {
      info: {},
      warning: {},
      warning_table: {},
      success: {},
      error: {},
    },

    variant: {
      subtle: {
        root: {
          color: 'alert.fg',
        },
      },
    },

    inline: {
      'true': {
        root: {
          alignItems: 'flex-start',
        },
        content: {
          display: 'inline-flex',
          flexDirection: 'row',
          alignItems: 'center',
        },
      },
      'false': {
        content: {
          display: 'flex',
          flexDirection: 'column',
        },
      },
    },

    size: {
      sm: {
        root: {
          gap: '2',
          px: '2',
          py: '2',
          textStyle: 'xs',
        },
        indicator: {
          boxSize: '5',
          my: 0,
        },
        content: {
          minH: '5',
        },
      },
      md: {
        root: {
          gap: '2',
          px: '3',
          py: '2',
          textStyle: 'md',
        },
        indicator: {
          boxSize: '5',
          my: '2px',
        },
      },
    },
  },

  compoundVariants: [
    {
      status: 'info',
      variant: 'subtle',
      css: {
        root: {
          bg: 'alert.bg.info',
        },
      },
    },
    {
      status: 'warning',
      variant: 'subtle',
      css: {
        root: { bg: 'alert.bg.warning' },
      },
    },
    {
      status: 'warning_table',
      variant: 'subtle',
      css: {
        root: { bg: 'alert.bg.warning_table' },
      },
    },
    {
      status: 'success',
      variant: 'subtle',
      css: {
        root: { bg: 'alert.bg.success' },
      },
    },
    {
      status: 'error',
      variant: 'subtle',
      css: {
        root: { bg: 'alert.bg.error' },
      },
    },
  ],

  defaultVariants: {
    status: 'info',
    size: 'md',
    inline: true,
    variant: 'subtle',
  },
});
