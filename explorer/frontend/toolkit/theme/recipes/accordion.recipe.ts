import { defineSlotRecipe } from '@chakra-ui/react';

export const recipe = defineSlotRecipe({
  className: 'chakra-accordion',
  slots: [ 'root', 'item', 'itemTrigger', 'itemContent', 'itemBody', 'itemIndicator' ],
  base: {
    root: {
      width: 'full',
      '--accordion-radius': 'none',
    },
    item: {
      overflowAnchor: 'none',
      borderColor: 'border.divider',
    },
    itemTrigger: {
      display: 'flex',
      alignItems: 'center',
      width: 'full',
      outline: '0',
      gap: '1',
      fontWeight: 'medium',
      borderRadius: 'var(--accordion-radius)',
      cursor: 'pointer',
      _focusVisible: {
        outline: '2px solid',
        outlineColor: 'colorPalette.focusRing',
      },
    },
    itemBody: {
      pt: '0',
      pb: 'var(--accordion-padding-y)',
    },
    itemContent: {
      overflow: 'hidden',
      borderRadius: 'var(--accordion-radius)',
      _open: {
        animationName: 'expand-height, fade-in',
        animationDuration: 'moderate',
      },
      _closed: {
        animationName: 'collapse-height, fade-out',
        animationDuration: 'moderate',
      },
    },
    itemIndicator: {
      transition: 'rotate 0.2s ease-in-out',
      transformOrigin: 'center',
    },
  },

  variants: {
    noAnimation: {
      'true': {
        itemContent: {
          _open: {
            animationName: 'none',
          },
          _closed: {
            animationName: 'none',
          },
        },
        itemIndicator: {
          transition: 'none',
        },
      },
    },
    variant: {
      outline: {
        item: {
          borderBottomWidth: '1px',
        },
        itemIndicator: {
          color: 'gray.500',
        },
      },
      faq: {
        item: {
          borderBottomWidth: '1px',
        },
        itemTrigger: {
          textStyle: 'heading.md',
        },
        itemIndicator: {
          color: 'link.primary',
          _groupHover: {
            color: 'link.primary.hover',
          },
        },
      },
    },

    size: {
      sm: {
        root: {
          '--accordion-padding-x': '0',
          '--accordion-padding-y': 'spacing.2',
        },
        itemTrigger: {
          textStyle: 'sm',
          py: 'var(--accordion-padding-y)',
        },
        itemIndicator: {
          boxSize: '5',
        },
      },
      md: {
        root: {
          '--accordion-padding-x': '0',
          '--accordion-padding-y': 'spacing.3',
        },
        itemTrigger: {
          textStyle: 'md',
          py: 'var(--accordion-padding-y)',
        },
        itemIndicator: {
          boxSize: '5',
        },
      },
    },
  },

  compoundVariants: [
    {
      variant: 'faq',
      size: 'md',
      css: {
        itemIndicator: {
          boxSize: '14px',
          margin: '5px',
        },
        itemBody: {
          paddingLeft: '36px',
        },
        itemTrigger: {
          gap: '3',
        },
      },
    },
  ],

  defaultVariants: {
    size: 'md',
    variant: 'outline',
  },
});
