import { defineSlotRecipe } from '@chakra-ui/react';

export const recipe = defineSlotRecipe({
  slots: [ 'content', 'item', 'itemText', 'itemGroupLabel', 'indicator', 'itemCommand', 'separator' ],
  base: {
    content: {
      outline: 0,
      bg: 'popover.bg',
      boxShadow: 'popover',
      color: 'initial',
      maxHeight: 'var(--available-height)',
      '--menu-z-index': 'zIndex.popover',
      zIndex: 'calc(var(--menu-z-index) + var(--layer-index, 0))',
      borderRadius: 'md',
      overflow: 'hidden',
      overflowY: 'auto',
      _open: {
        animationStyle: 'slide-fade-in',
        animationDuration: 'fast',
      },
      _closed: {
        animationStyle: 'slide-fade-out',
        animationDuration: 'faster',
      },
    },
    item: {
      textDecoration: 'none',
      color: 'text.primary',
      userSelect: 'none',
      borderRadius: 'none',
      width: '100%',
      display: 'flex',
      cursor: 'pointer',
      alignItems: 'center',
      textAlign: 'start',
      position: 'relative',
      flex: '0 0 auto',
      outline: 0,
      _disabled: {
        layerStyle: 'disabled',
      },
    },
    itemText: {
      flex: '1',
    },
    itemGroupLabel: {
      px: '2',
      py: '1.5',
      fontWeight: 'semibold',
      textStyle: 'sm',
    },
    indicator: {
      display: 'inline-flex',
      alignItems: 'center',
      justifyContent: 'center',
      flexShrink: '0',
    },
    itemCommand: {
      opacity: '0.6',
      textStyle: 'xs',
      ms: 'auto',
      ps: '4',
      letterSpacing: 'widest',
    },
    separator: {
      height: '1px',
      bg: 'bg.muted',
      my: '1',
      mx: '-1',
    },
  },

  variants: {
    variant: {
      subtle: {
        item: {
          _highlighted: {
            bg: 'selected.control.bg',
          },
        },
      },
    },

    size: {
      md: {
        content: {
          minW: '150px',
          py: '2',
          px: '0',
        },
        item: {
          gap: '2',
          textStyle: 'md',
          py: '2',
          px: '4',
        },
      },
    },
  },

  defaultVariants: {
    size: 'md',
    variant: 'subtle',
  },
});
