import { defineRecipe } from '@chakra-ui/react';

export const recipe = defineRecipe({
  base: {},

  variants: {
    loading: {
      // special value to override the default behavior of the skeleton in Chakra
      // it uses "background: unset" when the loading prop is set to false
      // but it causes issues with background color of child element (e.g. button, badge, etc.)
      // so, instead of the "loading" prop, we use the "state" prop to control the skeleton (see below)
      reset: {},
    },
    state: {
      loading: {
        borderRadius: 'base',
        boxShadow: 'none',
        backgroundClip: 'padding-box',
        cursor: 'default',
        color: 'transparent',
        borderWidth: '0px',
        pointerEvents: 'none',
        userSelect: 'none',
        '& > *': {
          visibility: 'hidden',
        },
      },
    },
    variant: {
      pulse: {
        background: 'bg.emphasized',
        animation: 'pulse',
        animationDuration: 'var(--duration, 1.2s)',
      },
      shine: {
        opacity: 0.5,
        '--animate-from': '100%',
        '--animate-to': '-100%',
        '--start-color': 'colors.skeleton.bg.start',
        '--end-color': 'colors.skeleton.bg.end',
        position: 'relative',
        overflow: 'hidden',
        backgroundColor: 'var(--start-color)',
        animation: 'none',
        '&::after': {
          content: '""',
          position: 'absolute',
          inset: '0',
          left: '0',
          width: '100%',
          pointerEvents: 'none',
          willChange: 'transform',
          transform: 'translateX(-100%)',
          backgroundImage:
            'linear-gradient(90deg, transparent 0%, transparent 32%, var(--end-color) 46%, var(--end-color) 54%, transparent 68%, transparent 100%)',
          animation: 'skeletonShimmer var(--duration, 2s) linear infinite',
        },
        _motionReduce: {
          '&::after': {
            animation: 'none',
          },
        },
      },
      none: {
        animation: 'none',
      },
    },
  },

  defaultVariants: {
    variant: 'shine',
    loading: 'reset',
  },
});
