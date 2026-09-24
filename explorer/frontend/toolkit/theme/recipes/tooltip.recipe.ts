import { defineSlotRecipe } from '@chakra-ui/react';

export const recipe = defineSlotRecipe({
  slots: [ 'content', 'arrow', 'arrowTip' ],
  base: {
    content: {
      px: '2',
      py: '1',
      borderRadius: 'sm',
      fontWeight: '500',
      textStyle: 'sm',
      textAlign: 'center',
      boxShadow: 'size.md',
      zIndex: 'tooltip',
      maxW: { base: 'calc(100vw - 8px)', lg: '320px' },
      transformOrigin: 'var(--transform-origin)',
      _open: {
        animationStyle: 'scale-fade-in',
        animationDuration: 'fast',
      },
      _closed: {
        animationStyle: 'scale-fade-out',
        animationDuration: 'fast',
      },
    },
    arrow: {
      '--arrow-size': 'sizes.2',
      '--arrow-background': 'var(--tooltip-bg)',
    },
    arrowTip: {
      borderTopWidth: '1px',
      borderInlineStartWidth: '1px',
      borderColor: 'var(--tooltip-bg)',
    },
  },
  variants: {
    variant: {
      regular: {
        content: {
          '--tooltip-bg': 'colors.tooltip.bg',
          bg: 'var(--tooltip-bg)',
          color: 'tooltip.fg',
        },
      },
      popover: {
        content: {
          maxW: 'none',
          bg: 'popover.bg',
          color: 'text.primary',
          p: '4',
          boxShadow: 'popover',
          boxShadowColor: 'popover.shadow',
          borderRadius: 'md',
          textAlign: 'left',
          fontWeight: 'normal',
        },
      },
    },
  },
  defaultVariants: {
    variant: 'regular',
  },
});
