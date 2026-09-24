import { defineSlotRecipe } from '@chakra-ui/react';

export const recipe = defineSlotRecipe({
  slots: [ 'root', 'circle', 'circleTrack', 'circleRange', 'label', 'valueText' ],
  base: {
    root: {
      display: 'inline-flex',
      textStyle: 'sm',
      position: 'relative',
    },
    circle: {
      _indeterminate: {
        animation: 'spin 2s linear infinite',
      },
    },
    circleTrack: {
      '--track-color': 'colors.progressCircle.track',
      stroke: 'var(--track-color)',
    },
    circleRange: {
      stroke: 'selected.option.bg',
      transitionProperty: 'stroke-dasharray',
      transitionDuration: '0.6s',
      _indeterminate: {
        animation: 'circular-progress 1.5s linear infinite',
      },
    },
    label: {
      display: 'inline-flex',
    },
    valueText: {
      lineHeight: '1',
      fontWeight: 'medium',
      letterSpacing: 'tight',
      fontVariantNumeric: 'tabular-nums',
    },
  },

  variants: {
    size: {
      sm: {
        circle: {
          '--size': '20px',
          '--thickness': '2px',
        },
        valueText: {
          textStyle: 'xs',
        },
      },
      md: {
        circle: {
          '--size': '32px',
          '--thickness': '3px',
        },
        valueText: {
          textStyle: 'xs',
        },
      },
      lg: {
        circle: {
          '--size': '80px',
          '--thickness': '8px',
        },
        valueText: {
          textStyle: 'sm',
        },
      },
    },
  },

  defaultVariants: {
    size: 'md',
  },
});
