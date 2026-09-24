import { defineSlotRecipe } from '@chakra-ui/react';

export const recipe = defineSlotRecipe({
  slots: [ 'root', 'title', 'description', 'indicator', 'actionTrigger', 'closeTrigger' ],
  base: {
    root: {
      width: 'full',
      display: 'flex',
      alignItems: 'flex-start',
      position: 'relative',
      gap: '3',
      py: '3',
      ps: '6',
      pe: '3',
      borderRadius: 'md',
      translate: 'var(--x) var(--y)',
      scale: 'var(--scale)',
      zIndex: 'var(--z-index)',
      height: 'var(--height)',
      opacity: 'var(--opacity)',
      willChange: 'translate, opacity, scale',
      transition:
        'translate 400ms, scale 400ms, opacity 400ms, height 400ms, box-shadow 200ms',
      transitionTimingFunction: 'cubic-bezier(0.21, 1.02, 0.73, 1)',
      _closed: {
        transition: 'translate 400ms, scale 400ms, opacity 200ms',
        transitionTimingFunction: 'cubic-bezier(0.06, 0.71, 0.55, 1)',
      },
      bg: 'toast.bg.info',
      color: 'toast.fg',
      boxShadow: 'xl',
      '--toast-trigger-bg': 'colors.bg.muted',
      '&[data-type=warning]': {
        color: 'toast.fg',
        bg: 'toast.bg.warning',
        '--toast-trigger-bg': '{white/10}',
        '--toast-border-color': '{white/40}',
      },
      '&[data-type=success]': {
        color: 'toast.fg',
        bg: 'toast.bg.success',
        '--toast-trigger-bg': '{white/10}',
        '--toast-border-color': '{white/40}',
      },
      '&[data-type=error]': {
        color: 'toast.fg',
        bg: 'toast.bg.error',
        '--toast-trigger-bg': '{white/10}',
        '--toast-border-color': '{white/40}',
      },
      '&[data-type=info]': {
        color: 'toast.fg',
        bg: 'toast.bg.info',
        '--toast-trigger-bg': '{white/10}',
        '--toast-border-color': '{white/40}',
      },
      '&[data-type=loading]': {
        color: 'toast.fg',
        bg: 'toast.bg.info',
        '--toast-trigger-bg': '{white/10}',
        '--toast-border-color': '{white/40}',
      },
    },
    title: {
      fontWeight: '700',
      textStyle: 'md',
      marginEnd: '0',
    },
    description: {
      display: 'inline',
      textStyle: 'md',
    },
    indicator: {
      flexShrink: '0',
      boxSize: '5',
    },
    actionTrigger: {
      textStyle: 'sm',
      fontWeight: 'medium',
      height: '8',
      px: '3',
      borderRadius: 'base',
      alignSelf: 'center',
      borderWidth: '1px',
      borderColor: 'var(--toast-border-color, inherit)',
      transition: 'background 200ms',
      _hover: {
        bg: 'var(--toast-trigger-bg)',
      },
    },
    closeTrigger: {
      position: 'static',
      alignSelf: 'flex-start',
      color: 'closeButton.fg',
      m: 0.5,
    },
  },
});
