import { defineSlotRecipe } from '@chakra-ui/react';

export const recipe = defineSlotRecipe({
  slots: [ 'root', 'label', 'helpText', 'valueUnit', 'valueText', 'indicator' ],
  base: {
    root: {
      display: 'flex',
      flexDirection: 'column',
      gap: '1',
      position: 'relative',
      flex: '1',
    },
    label: {
      display: 'inline-flex',
      gap: '1.5',
      alignItems: 'center',
      color: 'text.primary',
      textStyle: 'sm',
    },
    helpText: {
      color: 'text.primary',
      textStyle: 'xs',
    },
    valueUnit: {
      color: 'text.primary',
      textStyle: 'xs',
      fontWeight: 'initial',
      letterSpacing: 'initial',
    },
    valueText: {
      verticalAlign: 'baseline',
      fontWeight: '500',
      letterSpacing: 'normal',
      fontFeatureSettings: 'initial',
      fontVariantNumeric: 'initial',
      display: 'inline-flex',
      gap: '1',
    },
    indicator: {
      display: 'inline-flex',
      alignItems: 'center',
      justifyContent: 'center',
      marginEnd: 0,
      '& :where(svg)': {
        w: '1em',
        h: '1em',
      },
      '&[data-type=up]': {
        color: 'stat.indicator.up',
      },
      '&[data-type=down]': {
        color: 'stat.indicator.down',
      },
    },
  },

  variants: {
    orientation: {
      horizontal: {
        root: {
          flexDirection: 'row',
          alignItems: 'center',
        },
      },
    },
    positive: {
      'true': {
        valueText: {
          color: 'stat.indicator.up',
        },
      },
      'false': {
        valueText: {
          color: 'stat.indicator.down',
        },
      },
    },
    size: {
      sm: {
        valueText: {
          textStyle: 'sm',
        },
      },
      md: {
        valueText: {
          textStyle: 'md',
        },
      },
      lg: {
        valueText: {
          textStyle: 'lg',
        },
      },
    },
  },

  defaultVariants: {
    size: 'sm',
    orientation: 'horizontal',
  },
});
