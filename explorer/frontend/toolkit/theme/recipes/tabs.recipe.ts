import { defineSlotRecipe } from '@chakra-ui/react';

export const recipe = defineSlotRecipe({
  slots: [ 'root', 'list', 'trigger', 'content', 'indicator' ],
  base: {
    root: {
      '--tabs-trigger-radius': 'radii.l2',
      position: 'relative',
      _horizontal: {
        display: 'block',
      },
      _vertical: {
        display: 'flex',
      },
    },
    list: {
      display: 'inline-flex',
      width: '100%',
      position: 'relative',
      isolation: 'isolate',
      '--tabs-indicator-shadow': 'shadows.xs',
      '--tabs-indicator-bg': 'colors.bg',
      minH: 'var(--tabs-height)',
      _horizontal: {
        flexDirection: 'row',
      },
      _vertical: {
        flexDirection: 'column',
      },
    },
    trigger: {
      outline: '0',
      minW: 'var(--tabs-height)',
      height: 'var(--tabs-height)',
      display: 'flex',
      alignItems: 'center',
      position: 'relative',
      cursor: 'button',
      gap: '2',
      _focusVisible: {
        zIndex: 1,
        outline: '2px solid',
        outlineColor: 'colorPalette.focusRing',
      },
      _disabled: {
        cursor: 'not-allowed',
        opacity: 0.5,
      },
    },
    content: {
      focusVisibleRing: 'inside',
      _horizontal: {
        width: '100%',
        pt: 'var(--tabs-content-padding)',
      },
      _vertical: {
        height: '100%',
        ps: 'var(--tabs-content-padding)',
      },
    },
    indicator: {
      width: 'var(--width)',
      height: 'var(--height)',
      borderRadius: 'var(--tabs-indicator-radius)',
      bg: 'var(--tabs-indicator-bg)',
      shadow: 'var(--tabs-indicator-shadow)',
      zIndex: -1,
    },
  },

  variants: {
    fitted: {
      'true': {
        list: {
          display: 'flex',
        },
        trigger: {
          flex: 1,
          textAlign: 'center',
          justifyContent: 'center',
        },
      },
    },

    justify: {
      start: {
        list: {
          justifyContent: 'flex-start',
        },
      },
      center: {
        list: {
          justifyContent: 'center',
        },
      },
      end: {
        list: {
          justifyContent: 'flex-end',
        },
      },
    },

    size: {
      sm: {
        root: {
          '--tabs-height': 'sizes.8',
          '--tabs-content-padding': 'spacing.6',
        },
        trigger: {
          py: '1',
          px: '3',
          textStyle: 'sm',
        },
      },
      md: {
        root: {
          '--tabs-height': 'sizes.10',
          '--tabs-content-padding': 'spacing.6',
        },
        trigger: {
          py: '2',
          px: '4',
          textStyle: 'md',
        },
      },
      free: {},
    },

    variant: {
      solid: {
        trigger: {
          fontWeight: '600',
          gap: '1',
          borderRadius: 'base',
          color: 'tabs.solid.fg',
          bg: 'transparent',
          _selected: {
            bg: 'selected.control.bg',
            color: 'selected.control.text',
            _hover: {
              color: 'selected.control.text',
            },
          },
          _hover: {
            color: 'hover',
          },
        },
      },
      secondary: {
        list: {
          border: 'none',
          columnGap: '2',
          _horizontal: {
            _before: {
              display: 'none',
            },
          },
        },
        trigger: {
          fontWeight: '500',
          color: 'tabs.secondary.fg',
          bg: 'transparent',
          borderWidth: '2px',
          borderStyle: 'solid',
          borderColor: 'tabs.secondary.border',
          borderRadius: 'base',
          _selected: {
            bg: 'selected.control.bg',
            color: 'selected.control.text',
            borderColor: 'transparent',
            _hover: {
              borderColor: 'transparent',
            },
          },
          _hover: {
            color: 'hover',
            borderColor: 'hover',
          },
        },
      },
      segmented: {
        trigger: {
          color: 'tabs.segmented.fg',
          bg: 'transparent',
          borderWidth: '2px',
          borderStyle: 'solid',
          borderColor: 'selected.control.bg',
          _hover: {
            color: 'hover',
          },
          _selected: {
            color: 'selected.control.text',
            bg: 'selected.control.bg',
            borderColor: 'selected.control.bg',
            _hover: {
              color: 'selected.control.text',
            },
            '& + *': {
              borderLeftWidth: '0',
            },
          },
          _notLast: {
            borderRightWidth: '0',
            _selected: {
              borderRightWidth: '2px',
            },
          },
          _first: {
            borderTopLeftRadius: 'base',
            borderBottomLeftRadius: 'base',
          },
          _last: {
            borderTopRightRadius: 'base',
            borderBottomRightRadius: 'base',
          },
        },
      },
      unstyled: {},
    },
  },

  defaultVariants: {
    size: 'md',
    variant: 'solid',
  },
});
