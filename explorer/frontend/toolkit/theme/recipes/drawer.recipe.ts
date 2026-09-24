import { defineSlotRecipe } from '@chakra-ui/react';

export const recipe = defineSlotRecipe({
  slots: [ 'root', 'backdrop', 'trigger', 'content', 'header', 'body', 'footer', 'title', 'description', 'positioner' ],
  className: 'chakra-drawer',
  base: {
    backdrop: {
      bg: 'blackAlpha.800',
      pos: 'fixed',
      insetInlineStart: 0,
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
      insetInlineStart: 0,
      top: 0,
      zIndex: 'modal',
      overscrollBehaviorY: 'none',
    },
    content: {
      display: 'flex',
      flexDirection: 'column',
      position: 'relative',
      width: '100%',
      outline: 0,
      zIndex: 'modal',
      textStyle: 'sm',
      maxH: '100dvh',
      color: 'inherit',
      bg: 'drawer.bg',
      boxShadow: 'drawer',
      _open: {
        animationDuration: 'slowest',
        animationTimingFunction: 'ease-in-smooth',
      },
      _closed: {
        animationDuration: 'slower',
        animationTimingFunction: 'ease-in-smooth',
      },
    },
    header: {
      flex: 0,
      px: '6',
      pt: '6',
      pb: '4',
    },
    body: {
      p: '6',
      flex: '1',
      overflow: 'auto',
    },
    footer: {
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'flex-end',
      gap: '3',
      px: '6',
      pt: '2',
      pb: '4',
    },
    title: {
      textStyle: 'heading.md',
      pr: '8',
      my: '1',
      fontWeight: '500',
    },
    description: {
      color: 'fg.muted',
    },
  },

  variants: {
    size: {
      md: {
        content: {
          maxW: '300px',
        },
      },
    },

    placement: {
      start: {
        positioner: {
          justifyContent: 'flex-start',
        },
        content: {
          _open: {
            animationName: {
              base: 'slide-from-left-full, fade-in',
              _rtl: 'slide-from-right-full, fade-in',
            },
          },
          _closed: {
            animationName: {
              base: 'slide-to-left-full, fade-out',
              _rtl: 'slide-to-right-full, fade-out',
            },
          },
        },
      },

      end: {
        positioner: {
          justifyContent: 'flex-end',
        },
        content: {
          _open: {
            animationName: {
              base: 'slide-from-right-full, fade-in',
              _rtl: 'slide-from-left-full, fade-in',
            },
          },
          _closed: {
            animationName: {
              base: 'slide-to-right-full, fade-out',
              _rtl: 'slide-to-right-full, fade-out',
            },
          },
        },
      },

      top: {
        positioner: {
          alignItems: 'flex-start',
        },
        content: {
          maxW: '100%',
          _open: { animationName: 'slide-from-top-full, fade-in' },
          _closed: { animationName: 'slide-to-top-full, fade-out' },
        },
      },

      bottom: {
        positioner: {
          alignItems: 'flex-end',
        },
        header: {
          px: '5',
          pt: '5',
          pb: '3',
          mb: '-5',
        },
        body: {
          p: '5',
        },
        footer: {
          px: '5',
          pt: '3',
          mt: '-5',
        },
        content: {
          maxW: '100%',
          _open: { animationName: 'slide-from-bottom-full, fade-in' },
          _closed: { animationName: 'slide-to-bottom-full, fade-out' },
          borderTopRadius: 'base',
        },
      },
    },

    contained: {
      'true': {
        positioner: {
          padding: '4',
        },
        content: {
          borderRadius: 'l3',
        },
      },
    },
  },

  defaultVariants: {
    size: 'md',
    placement: 'end',
  },
});
