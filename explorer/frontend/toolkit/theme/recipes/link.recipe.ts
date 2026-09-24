import { defineRecipe } from '@chakra-ui/react';

export const recipe = defineRecipe({
  base: {
    gap: 0,
    _disabled: {
      cursor: 'not-allowed',
    },
  },
  variants: {
    variant: {
      primary: {
        color: 'link.primary',
        _hover: {
          textDecoration: 'none',
          color: 'link.primary.hover',
        },
      },
      secondary: {
        color: 'link.secondary',
        _hover: {
          textDecoration: 'none',
          color: 'hover',
        },
      },
      subtle: {
        color: 'link.subtle',
        _hover: {
          color: 'link.subtle.hover',
          textDecorationLine: 'underline',
          textDecorationColor: 'link.subtle.hover',
        },
      },
      underlaid: {
        color: 'link.primary',
        bgColor: 'link.underlaid.bg',
        px: '8px',
        py: '6px',
        borderRadius: 'base',
        textStyle: 'sm',
        _hover: {
          color: 'link.primary.hover',
          textDecoration: 'none',
        },
      },
      menu: {
        color: 'link.menu',
        _hover: {
          color: 'hover',
          textDecoration: 'none',
        },
      },
      navigation: {
        color: 'link.navigation.fg',
        bg: 'transparent',
        _hover: {
          color: 'link.navigation.fg.hover',
          textDecoration: 'none',
        },
        _selected: {
          color: 'link.navigation.fg.selected',
          bg: 'link.navigation.bg.selected',
        },
        _active: {
          color: 'link.navigation.fg.active',
        },
      },
      plain: {
        color: 'inherit',
        _hover: {
          textDecoration: 'none',
        },
      },
    },
  },
  defaultVariants: {
    variant: 'primary',
  },
});
