import { defineSlotRecipe } from '@chakra-ui/react';

export const recipe = defineSlotRecipe({
  slots: [ 'backdrop', 'positioner', 'content', 'header', 'body', 'footer', 'title', 'description' ],
  base: {
    backdrop: {
      bg: 'blackAlpha.800',
      pos: 'fixed',
      left: 0,
      top: 0,
      w: '100vw',
      h: '100dvh',
      zIndex: 'modal',
      _open: {
        animationName: 'fade-in',
        animationDuration: 'slow',
      },
      _closed: {
        animationName: 'fade-out',
        animationDuration: 'moderate',
      },
    },
    positioner: {
      display: 'flex',
      width: '100vw',
      height: '100dvh',
      position: 'fixed',
      left: 0,
      top: 0,
      '--dialog-z-index': 'zIndex.modal',
      zIndex: 'calc(var(--dialog-z-index) + var(--layer-index, 0))',
      justifyContent: 'center',
      overscrollBehaviorY: 'none',
    },
    content: {
      display: 'flex',
      flexDirection: 'column',
      position: 'relative',
      width: '100%',
      padding: 6,
      outline: 0,
      textStyle: 'md',
      my: 'var(--dialog-margin, var(--dialog-base-margin))',
      '--dialog-z-index': 'zIndex.modal',
      zIndex: 'calc(var(--dialog-z-index) + var(--layer-index, 0))',
      bg: 'dialog.bg',
      color: 'dialog.fg',
      boxShadow: 'size.lg',
      borderRadius: 'xl',
      _open: {
        animationDuration: 'moderate',
      },
      _closed: {
        animationDuration: 'faster',
      },
    },
    header: {
      flex: 0,
      p: 0,
      mb: 2,
      display: 'flex',
      alignItems: 'center',
      columnGap: 2,
      minH: '40px',
    },
    body: {
      flex: '1',
      p: 0,
    },
    footer: {
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'flex-start',
      gap: '6',
      p: '0',
      mt: '6',
    },
    title: {
      textStyle: { base: 'heading.md', lg: 'heading.lg' },
      whiteSpace: 'nowrap',
      overflow: 'hidden',
      textOverflow: 'ellipsis',
      fontWeight: '500',
    },
    description: {
      color: 'dialog.fg',
    },
  },

  variants: {
    placement: {
      center: {
        positioner: {
          alignItems: 'center',
        },
        content: {
          '--dialog-base-margin': 'auto',
          mx: 'auto',
        },
      },
      top: {
        positioner: {
          alignItems: 'flex-start',
        },
        content: {
          '--dialog-base-margin': 'spacing.16',
          mx: 'auto',
        },
      },
      bottom: {
        positioner: {
          alignItems: 'flex-end',
        },
        content: {
          '--dialog-base-margin': 'spacing.16',
          mx: 'auto',
        },
      },
    },

    scrollBehavior: {
      inside: {
        positioner: {
          overflow: 'hidden',
        },
        content: {
          // source code has minH: 'auto', but I am not sure why
          // anyway it will override the minH from the "full" size variant
          // minH: 'auto',
          maxH: 'calc(100% - 7.5rem)',
        },
        body: {
          overflow: 'auto',
        },
      },
      outside: {
        positioner: {
          overflow: 'auto',
          pointerEvents: 'auto',
        },
      },
    },

    size: {
      sm: {
        content: {
          maxW: '400px',
        },
      },
      md: {
        content: {
          maxW: '728px',
        },
      },
      cover: {
        positioner: {
          padding: '10',
        },
        content: {
          width: '100%',
          height: '100%',
          '--dialog-margin': '0',
        },
      },
      full: {
        content: {
          maxW: '100vw',
          minH: '100dvh',
          '--dialog-margin': '0',
          borderRadius: '0',
        },
      },
    },

    motionPreset: {
      scale: {
        content: {
          _open: { animationName: 'scale-in, fade-in' },
          _closed: { animationName: 'scale-out, fade-out' },
        },
      },
      'slide-in-bottom': {
        content: {
          _open: { animationName: 'slide-from-bottom, fade-in' },
          _closed: { animationName: 'slide-to-bottom, fade-out' },
        },
      },
      'slide-in-top': {
        content: {
          _open: { animationName: 'slide-from-top, fade-in' },
          _closed: { animationName: 'slide-to-top, fade-out' },
        },
      },
      'slide-in-left': {
        content: {
          _open: { animationName: 'slide-from-left, fade-in' },
          _closed: { animationName: 'slide-to-left, fade-out' },
        },
      },
      'slide-in-right': {
        content: {
          _open: { animationName: 'slide-from-right, fade-in' },
          _closed: { animationName: 'slide-to-right, fade-out' },
        },
      },
      none: {},
    },
  },

  defaultVariants: {
    size: 'md',
    scrollBehavior: 'inside',
    placement: { base: 'top', lg: 'center' },
    motionPreset: 'scale',
  },
});
