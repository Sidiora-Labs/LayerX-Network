import { defineSlotRecipe } from '@chakra-ui/react';

export const recipe = defineSlotRecipe({
  slots: [
    'root',
    'trigger',
    'indicatorGroup',
    'indicator',
    'content',
    'item',
    'control',
    'itemText',
    'itemGroup',
    'itemGroupLabel',
    'label',
    'valueText',
    'positioner',
  ],
  base: {
    root: {
      display: 'flex',
      flexDirection: 'column',
      gap: '1.5',
      width: '100%',
    },
    trigger: {
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'flex-start',
      width: 'full',
      minH: 'var(--select-trigger-height)',
      pr: 'var(--select-trigger-padding-right)',
      pl: 'var(--select-trigger-padding-left)',
      borderRadius: 'base',
      userSelect: 'none',
      textAlign: 'start',
      fontWeight: 'medium',
      cursor: 'pointer',
      focusVisibleRing: 'none',
      _disabled: {
        opacity: 'control.disabled',
      },
      _placeholderShown: {
        '& [data-part=value-text]': {
          display: '-webkit-box',
        },
      },
    },
    indicatorGroup: {
      display: 'flex',
      alignItems: 'center',
      gap: '1',
      pos: 'absolute',
      right: '0',
      top: '0',
      bottom: '0',
      px: '0',
      pointerEvents: 'none',
    },
    indicator: {
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      boxSize: '5',
      color: 'inherit',
      _open: {
        color: 'hover',
      },
    },
    content: {
      background: 'popover.bg',
      display: 'flex',
      flexDirection: 'column',
      // For selects inside modals
      '--select-z-index': 'modal',
      zIndex: 'modal',
      borderRadius: 'md',
      borderWidth: '0',
      outline: 0,
      boxShadow: 'popover',
      boxShadowColor: 'colors.popover.shadow',
      maxH: '96',
      overflowY: 'auto',
      minWidth: '150px',
      rowGap: '2',
      _open: {
        animationStyle: 'slide-fade-in',
        animationDuration: 'fast',
      },
      _closed: {
        animationStyle: 'slide-fade-out',
        animationDuration: 'fastest',
      },
    },
    item: {
      position: 'relative',
      userSelect: 'none',
      display: 'flex',
      alignItems: 'center',
      gap: '2',
      cursor: 'pointer',
      justifyContent: 'flex-start',
      flex: '1',
      textAlign: 'start',
      borderRadius: 'none',
      _disabled: {
        pointerEvents: 'none',
        opacity: 'control.disabled',
      },
      _highlighted: {
        bg: 'selected.control.bg',
      },
    },
    control: {
      pos: 'relative',
    },
    itemText: {
      flex: '1',
    },
    itemGroup: {
      _first: { mt: '0' },
    },
    itemGroupLabel: {
      py: '1',
      fontWeight: 'medium',
    },
    label: {
      fontWeight: 'medium',
      userSelect: 'none',
      textStyle: 'sm',
      _disabled: {
        opacity: 'control.disabled',
      },
    },
    valueText: {
      display: 'flex',
      flexDirection: 'column',
      lineClamp: '1',
      maxW: '100%',
      wordBreak: 'break-all',
    },
  },

  variants: {
    variant: {
      outline: {
        trigger: {
          borderWidth: '2px',
          bg: 'input.bg',
          color: 'select.trigger.outline.fg',
          borderColor: 'input.border.filled',
          _expanded: {
            color: 'hover',
            borderColor: 'hover',
            _hover: {
              color: 'hover',
              borderColor: 'hover',
            },
          },
          _hover: {
            color: 'select.trigger.outline.fg',
            borderColor: 'input.border.hover',
          },
          _focusVisible: {
            borderColor: 'input.border.focus',
          },
          _readOnly: {
            userSelect: 'all',
            pointerEvents: 'none',
            cursor: 'default',
            bg: 'input.bg.readOnly',
            borderColor: 'input.border.readOnly',
            _focus: {
              borderColor: 'input.border.readOnly',
            },
            _hover: {
              borderColor: 'input.border.readOnly',
            },
          },
          _invalid: {
            borderColor: 'input.border.error',
            _hover: {
              borderColor: 'input.border.error',
            },
            _expanded: {
              color: 'hover',
              borderColor: 'hover',
              _hover: {
                color: 'hover',
                borderColor: 'hover',
              },
            },
          },
          _placeholderShown: {
            color: 'select.placeholder.fg',
            borderColor: 'input.border',
            _hover: {
              color: 'select.placeholder.fg',
            },
            _invalid: {
              color: 'select.placeholder.fg.error',
              _hover: {
                color: 'select.placeholder.fg.error',
              },
            },
          },
        },
        indicatorGroup: {
          color: 'select.indicator.fg',
          _peerDisabled: {
            opacity: 'control.disabled',
          },
        },
      },
      plain: {
        trigger: {},
        indicatorGroup: {},
      },
    },

    size: {
      sm: {
        root: {
          '--select-trigger-height': 'sizes.8',
          '--select-trigger-padding-right': 'spacing.8',
          '--select-trigger-padding-left': 'spacing.2',
        },
        content: {
          px: '0',
          py: '4',
          textStyle: 'md',
        },
        trigger: {
          textStyle: 'sm',
          gap: '1',
        },
        indicatorGroup: {
          pr: '2',
          pl: '1',
        },
        item: {
          py: '5px',
          px: '4',
        },
      },
      lg: {
        root: {
          '--select-trigger-height': '60px',
          '--select-trigger-padding-right': '44px',
          '--select-trigger-padding-left': 'spacing.4',
        },
        content: {
          px: '0',
          py: '4',
          textStyle: 'md',
        },
        trigger: {
          py: '2',
        },
        item: {
          py: '5px',
          px: '4',
        },
        indicatorGroup: {
          pr: '4',
          pl: '2',
        },
      },
    },
  },

  compoundVariants: [
    {
      size: 'sm',
      variant: 'outline',
      css: {
        trigger: {
          _placeholderShown: {
            color: 'select.trigger.outline.fg',
            _hover: {
              color: 'select.trigger.outline.fg',
            },
          },
        },
      },
    },
  ],

  defaultVariants: {
    size: 'sm',
    variant: 'outline',
  },
});
