import { defineRecipe } from '@chakra-ui/react';

export const recipe = defineRecipe({
  base: {
    display: 'inline-flex',
    alignItems: 'center',
    borderRadius: 'sm',
    gap: '1',
    fontWeight: '500',
    width: 'fit-content',
    maxWidth: '100%',
    whiteSpace: 'nowrap',
    fontVariantNumeric: 'normal',
    userSelect: 'text',
    _loading: {
      borderRadius: 'sm',
    },
  },
  variants: {
    variant: {
      subtle: {},
    },
    colorPalette: {
      gray: {
        bg: 'badge.gray.bg',
        color: 'badge.gray.fg',
      },
      green: {
        bg: 'badge.green.bg',
        color: 'badge.green.fg',
      },
      red: {
        bg: 'badge.red.bg',
        color: 'badge.red.fg',
      },
      purple: {
        bg: 'badge.purple.bg',
        color: 'badge.purple.fg',
      },
      orange: {
        bg: 'badge.orange.bg',
        color: 'badge.orange.fg',
      },
      blue: {
        bg: 'badge.blue.bg',
        color: 'badge.blue.fg',
      },
      yellow: {
        bg: 'badge.yellow.bg',
        color: 'badge.yellow.fg',
      },
      teal: {
        bg: 'badge.teal.bg',
        color: 'badge.teal.fg',
      },
      cyan: {
        bg: 'badge.cyan.bg',
        color: 'badge.cyan.fg',
      },
      pink: {
        bg: 'badge.pink.bg',
        color: 'badge.pink.fg',
      },
      purple_alt: {
        bg: 'badge.purple_alt.bg',
        color: 'badge.purple_alt.fg',
      },
      blue_alt: {
        bg: 'badge.blue_alt.bg',
        color: 'badge.blue_alt.fg',
      },
      // bright badges mainly used in other projects (e.g. autoscout, dev portal, etc.)
      bright_gray: {
        bg: 'badge.bright.gray.bg',
        color: 'badge.bright.gray.fg',
      },
      bright_green: {
        bg: 'badge.bright.green.bg',
        color: 'badge.bright.green.fg',
      },
      bright_red: {
        bg: 'badge.bright.red.bg',
        color: 'badge.bright.red.fg',
      },
      bright_blue: {
        bg: 'badge.bright.blue.bg',
        color: 'badge.bright.blue.fg',
      },
      bright_yellow: {
        bg: 'badge.bright.yellow.bg',
        color: 'badge.bright.yellow.fg',
      },
      bright_teal: {
        bg: 'badge.bright.teal.bg',
        color: 'badge.bright.teal.fg',
      },
      bright_cyan: {
        bg: 'badge.bright.cyan.bg',
        color: 'badge.bright.cyan.fg',
      },
      bright_orange: {
        bg: 'badge.bright.orange.bg',
        color: 'badge.bright.orange.fg',
      },
      bright_purple: {
        bg: 'badge.bright.purple.bg',
        color: 'badge.bright.purple.fg',
      },
      bright_pink: {
        bg: 'badge.bright.pink.bg',
        color: 'badge.bright.pink.fg',
      },
    },
    size: {
      sm: {
        textStyle: 'xs',
        p: '1',
        h: '4.5',
        minH: '4.5',
      },
      md: {
        textStyle: 'sm',
        px: '1',
        py: '0.5',
        minH: '6',
      },
      lg: {
        textStyle: 'sm',
        px: '2',
        py: '1',
        minH: '7',
        fontWeight: '600',
      },
    },
  },
  defaultVariants: {
    variant: 'subtle',
    colorPalette: 'gray',
    size: 'md',
  },
});
