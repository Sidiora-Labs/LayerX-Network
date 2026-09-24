import { defineRecipe } from '@chakra-ui/react';

export const recipe = defineRecipe({
  base: {
    width: '100%',
    minWidth: '0',
    minHeight: '160px',
    outline: '0',
    position: 'relative',
    appearance: 'none',
    textAlign: 'start',
    borderRadius: 'base',
    color: 'input.fg',
    '--focus-color': 'colors.border.error',
    '--error-color': 'colors.border.error',
    _invalid: {
      focusRingColor: 'var(--error-color)',
      borderColor: 'var(--error-color)',
    },
  },
  variants: {
    size: {
      '2xl': {
        textStyle: 'md',
        py: '4',
        pl: '4',
        pr: '5', // === scrollbar width
        scrollPaddingBottom: '4',
      },
    },

    variant: {
      outline: {
        bg: 'input.bg',
        borderWidth: '2px',
        borderColor: 'input.border',
        focusVisibleRing: 'none',
        _hover: {
          borderColor: 'input.border.hover',
        },
        _focus: {
          borderColor: 'input.border.focus',
          boxShadow: 'size.md',
          _hover: {
            borderColor: 'input.border.focus',
          },
        },
        _readOnly: {
          userSelect: 'all',
          bg: 'input.bg.readOnly',
          borderColor: 'input.border.readOnly',
          _focus: {
            borderColor: 'input.border.readOnly',
          },
          _hover: {
            borderColor: 'input.border.readOnly',
          },
        },
        _disabled: {
          pointerEvents: 'none',
          opacity: 'control.disabled',
        },
        _invalid: {
          borderColor: 'input.border.error',
          _hover: {
            borderColor: 'input.border.error',
          },
        },
      },
    },

    floating: {
      'true': {
        paddingTop: '8',
        _placeholderShown: {
          paddingTop: '10',
        },
      },
    },
  },

  defaultVariants: {
    size: '2xl',
    variant: 'outline',
  },
});
