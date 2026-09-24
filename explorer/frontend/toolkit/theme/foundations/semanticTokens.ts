import type { ThemingConfig } from '@chakra-ui/react';

import config from 'configs/app';

const heroBannerButton = config.UI.homepage.heroBanner?.button;

const semanticTokens: ThemingConfig['semanticTokens'] = {
  colors: {
    // COMMON STATES
    hover: {
      DEFAULT: { value: { _light: '{colors.theme.hover._light}', _dark: '{colors.theme.hover._dark}' } },
    },
    selected: {
      control: {
        bg: { value: { _light: '{colors.theme.selected.control.bg._light}', _dark: '{colors.theme.selected.control.bg._dark}' } },
        text: { value: { _light: '{colors.theme.selected.control.text._light}', _dark: '{colors.theme.selected.control.text._dark}' } },
      },
      option: {
        bg: { value: { _light: '{colors.theme.selected.option.bg._light}', _dark: '{colors.theme.selected.option.bg._dark}' } },
      },
    },

    // GLOBAL
    global: {
      body: {
        bg: { value: '{colors.bg.primary}' },
        fg: { value: '{colors.text.primary}' },
      },
      mark: {
        bg: { value: { _light: '{colors.green.100}', _dark: '{colors.green.800}' } },
      },
      scrollbar: {
        thumb: { value: { _light: '{colors.blackAlpha.300}', _dark: '{colors.whiteAlpha.300}' } },
      },
      selection: {
        bg: { value: { _light: '#E3CFE7', _dark: '#754B7D' } },
      },
    },

    // FOUNDATIONS
    heading: {
      DEFAULT: { value: { _light: '{colors.blackAlpha.800}', _dark: '{colors.whiteAlpha.800}' } },
    },
    text: {
      primary: { value: { _light: '{colors.theme.text.primary._light}', _dark: '{colors.theme.text.primary._dark}' } },
      secondary: { value: { _light: '{colors.theme.text.secondary._light}', _dark: '{colors.theme.text.secondary._dark}' } },
      error: { value: '{colors.red.500}' },
      success: { value: { _light: '{colors.green.500}', _dark: '{colors.green.200}' } },
    },
    bg: {
      primary: { value: { _light: '{colors.theme.bg.primary._light}', _dark: '{colors.theme.bg.primary._dark}' } },
    },
    border: {
      divider: { value: { _light: '{colors.blackAlpha.100}', _dark: '{colors.whiteAlpha.100}' } },
      error: { value: '{colors.red.500}' },
    },
    icon: {
      primary: { value: { _light: '{colors.theme.icon.primary._light}', _dark: '{colors.theme.icon.primary._dark}' } },
      secondary: { value: { _light: '{colors.theme.icon.secondary._light}', _dark: '{colors.theme.icon.secondary._dark}' } },
    },

    // ELEMENTS
    address: {
      highlighted: {
        bg: { value: { _light: '{colors.blue.50}', _dark: '{colors.blue.900}' } },
        border: { value: { _light: '{colors.blue.200}', _dark: '{colors.blue.600}' } },
      },
    },

    // COMPONENTS
    button: {
      solid: {
        bg: {
          DEFAULT: { value: { _light: '{colors.theme.button.primary._light}', _dark: '{colors.theme.button.primary._dark}' } },
        },
        text: {
          DEFAULT: { value: { _light: '{colors.theme.button.primary.text._light}', _dark: '{colors.theme.button.primary.text._dark}' } },
        },
      },
      outline: {
        fg: {
          DEFAULT: { value: { _light: '{colors.theme.button.primary._light}', _dark: '{colors.theme.button.primary._dark}' } },
        },
      },
      subtle: {
        fg: {
          DEFAULT: { value: { _light: '{colors.blackAlpha.800}', _dark: '{colors.whiteAlpha.800}' } },
        },
        bg: {
          DEFAULT: { value: { _light: '{colors.blackAlpha.200}', _dark: '{colors.whiteAlpha.200}' } },
        },
      },
      dropdown: {
        fg: {
          DEFAULT: { value: { _light: '{colors.blackAlpha.800}', _dark: '{colors.whiteAlpha.800}' } },
        },
        border: {
          DEFAULT: { value: { _light: '{colors.gray.100}', _dark: '{colors.gray.600}' } },
        },
      },
      header: {
        fg: {
          DEFAULT: { value: { _light: '{colors.blackAlpha.800}', _dark: '{colors.gray.400}' } },
          selected: { value: { _light: '{colors.blackAlpha.800}', _dark: '{colors.whiteAlpha.800}' } },
          highlighted: { value: { _light: '{colors.blackAlpha.800}', _dark: '{colors.whiteAlpha.800}' } },
        },
        bg: {
          selected: { value: { _light: '{colors.blackAlpha.50}', _dark: '{colors.whiteAlpha.100}' } },
          highlighted: { value: { _light: '{colors.orange.100}', _dark: '{colors.orange.900}' } },
        },
        border: {
          DEFAULT: { value: { _light: '{colors.gray.100}', _dark: '{colors.gray.600}' } },
        },
      },
      segmented: {
        fg: {
          DEFAULT: { value: '{colors.text.primary}' },
        },
      },
      icon_background: {
        bg: {
          DEFAULT: { value: { _light: '{colors.gray.50}', _dark: '{colors.whiteAlpha.50}' } },
        },
      },
      pagination: {
        fg: {
          DEFAULT: { value: { _light: '{colors.blackAlpha.800}', _dark: '{colors.gray.50}' } },
        },
        border: {
          DEFAULT: { value: { _light: '{colors.gray.100}', _dark: '{colors.whiteAlpha.100}' } },
        },
      },
      hero: {
        bg: {
          DEFAULT: {
            value: {
              _light: heroBannerButton?._default?.background?.[0] || '{colors.theme.button.primary._light}',
              _dark: heroBannerButton?._default?.background?.[1] || heroBannerButton?._default?.background?.[0] || '{colors.theme.button.primary._dark}',
            },
          },
          hover: {
            value: {
              _light: heroBannerButton?._hover?.background?.[0] || '{colors.hover}',
              _dark: heroBannerButton?._hover?.background?.[1] || heroBannerButton?._hover?.background?.[0] || '{colors.hover}',
            },
          },
          selected: {
            value: {
              _light: heroBannerButton?._selected?.background?.[0] || '{colors.blue.50}',
              _dark: heroBannerButton?._selected?.background?.[1] || heroBannerButton?._selected?.background?.[0] || '{colors.blue.50}',
            },
          },
        },
        fg: {
          DEFAULT: {
            value: {
              _light: heroBannerButton?._default?.text_color?.[0] || '{colors.white}',
              _dark: heroBannerButton?._default?.text_color?.[1] || heroBannerButton?._default?.text_color?.[0] || '{colors.white}',
            },
          },
          hover: {
            value: {
              _light: heroBannerButton?._hover?.text_color?.[0] || '{colors.white}',
              _dark: heroBannerButton?._hover?.text_color?.[1] || heroBannerButton?._hover?.text_color?.[0] || '{colors.white}',
            },
          },
          selected: {
            value: {
              _light: heroBannerButton?._selected?.text_color?.[0] || '{colors.blackAlpha.800}',
              _dark: heroBannerButton?._selected?.text_color?.[1] || heroBannerButton?._selected?.text_color?.[0] || '{colors.blackAlpha.800}',
            },
          },
        },
      },
    },
    closeButton: {
      fg: {
        DEFAULT: { value: { _light: '{colors.blackAlpha.500}', _dark: '{colors.whiteAlpha.500}' } },
      },
    },
    link: {
      primary: {
        DEFAULT: { value: { _light: '{colors.theme.link.primary._light}', _dark: '{colors.theme.link.primary._dark}' } },
        hover: { value: '{colors.hover}' },
      },
      secondary: {
        DEFAULT: { value: '{colors.text.secondary}' },
      },
      underlaid: {
        bg: { value: { _light: '{colors.gray.100}', _dark: '{colors.gray.800}' } },
      },
      subtle: {
        DEFAULT: { value: { _light: '{colors.blackAlpha.800}', _dark: '{colors.gray.400}' } },
        hover: { value: { _light: '{colors.blackAlpha.800}', _dark: '{colors.gray.400}' } },
      },
      navigation: {
        fg: {
          DEFAULT: { value: '{colors.text.primary}' },
          selected: { value: { _light: '{colors.theme.navigation.text.selected._light}', _dark: '{colors.theme.navigation.text.selected._dark}' } },
          hover: { value: { _light: '{colors.hover}' } },
          active: { value: { _light: '{colors.hover}' } },
        },
        bg: {
          selected: { value: { _light: '{colors.theme.navigation.bg.selected._light}', _dark: '{colors.theme.navigation.bg.selected._dark}' } },
          group: { value: { _light: '{colors.white}', _dark: '{colors.black}' } },
        },
      },
      menu: {
        DEFAULT: { value: { _light: '{colors.blackAlpha.800}', _dark: '{colors.whiteAlpha.800}' } },
      },
    },
    tooltip: {
      DEFAULT: {
        bg: { value: '{colors.gray.900}' },
        fg: { value: '{colors.white}' },
      },
    },
    popover: {
      DEFAULT: {
        bg: { value: { _light: '{colors.white}', _dark: '{colors.gray.900}' } },
        shadow: { value: { _light: '{colors.blackAlpha.200}', _dark: '{colors.whiteAlpha.300}' } },
      },
    },
    progress: {
      track: {
        DEFAULT: { value: { _light: '{colors.gray.100}', _dark: '{colors.whiteAlpha.100}' } },
      },
    },
    progressCircle: {
      track: {
        DEFAULT: { value: { _light: '{colors.gray.100}', _dark: '{colors.whiteAlpha.100}' } },
      },
    },
    skeleton: {
      bg: {
        start: { value: { _light: '{colors.blackAlpha.50}', _dark: '{colors.whiteAlpha.50}' } },
        end: { value: { _light: '{colors.blackAlpha.100}', _dark: '{colors.whiteAlpha.100}' } },
      },
    },
    tabs: {
      solid: {
        fg: {
          DEFAULT: { value: { _light: '{colors.theme.tabs.text.primary._light}', _dark: '{colors.theme.tabs.text.primary._dark}' } },
        },
      },
      secondary: {
        fg: {
          DEFAULT: { value: '{colors.text.primary}' },
        },
        border: {
          DEFAULT: { value: { _light: '{colors.gray.300}', _dark: '{colors.gray.600}' } },
        },
      },
      segmented: {
        fg: {
          DEFAULT: { value: '{colors.text.primary}' },
        },
      },
    },
    'switch': {
      primary: {
        bg: {
          DEFAULT: { value: { _light: '{colors.gray.300}', _dark: '{colors.whiteAlpha.400}' } },
        },
      },
    },
    alert: {
      fg: {
        DEFAULT: { value: { _light: '{colors.blackAlpha.800}', _dark: '{colors.whiteAlpha.800}' } },
      },
      bg: {
        info: { value: { _light: '{colors.blackAlpha.50}', _dark: '{colors.whiteAlpha.100}' } },
        warning: { value: { _light: '{colors.orange.100}', _dark: '{colors.orange.800/44}' } },
        warning_table: { value: { _light: '{colors.orange.50}', _dark: '{colors.orange.800/44}' } },
        success: { value: { _light: '{colors.green.100}', _dark: '{colors.green.900}' } },
        error: { value: { _light: '{colors.red.100}', _dark: '{colors.red.900}' } },
      },
    },
    toast: {
      fg: {
        DEFAULT: { value: '{colors.alert.fg}' },
      },
      bg: {
        DEFAULT: { value: '{colors.alert.bg.info}' },
        info: { value: { _light: '{colors.blue.100}', _dark: '{colors.blue.900}' } },
        warning: { value: '{colors.alert.bg.warning}' },
        success: { value: '{colors.alert.bg.success}' },
        error: { value: '{colors.alert.bg.error}' },
        loading: { value: { _light: '{colors.blue.100}', _dark: '{colors.blue.900}' } },
      },
    },
    input: {
      fg: {
        DEFAULT: { value: { _light: '{colors.gray.800}', _dark: '{colors.gray.50}' } },
        error: { value: '{colors.text.error}' },
      },
      bg: {
        DEFAULT: { value: '{colors.bg.primary}' },
        readOnly: { value: { _light: '{colors.gray.200}', _dark: '{colors.gray.800}' } },
      },
      border: {
        DEFAULT: { value: { _light: '{colors.gray.100}', _dark: '{colors.gray.600}' } },
        hover: { value: { _light: '{colors.gray.200}', _dark: '{colors.gray.500}' } },
        focus: { value: '{colors.hover}' },
        filled: { value: { _light: '{colors.gray.100}', _dark: '{colors.gray.600}' } },
        readOnly: { value: { _light: '{colors.gray.200}', _dark: '{colors.gray.800}' } },
        error: { value: '{colors.red.500}' },
      },
      placeholder: {
        DEFAULT: { value: { _light: '{colors.gray.400}', _dark: '{colors.gray.500}' } },
        error: { value: '{colors.red.500}' },
      },
      element: {
        DEFAULT: { value: { _light: '{colors.gray.400}', _dark: '{colors.gray.500}' } },
      },
    },
    field: {
      placeholder: {
        DEFAULT: { value: { _light: '{colors.gray.400}', _dark: '{colors.gray.500}' } },
        disabled: { value: '{colors.gray.500/20}' },
        error: { value: '{colors.red.500}' },
      },
    },
    dialog: {
      bg: {
        DEFAULT: { value: { _light: '{colors.white}', _dark: '{colors.gray.900}' } },
      },
      fg: {
        DEFAULT: { value: { _light: '{colors.blackAlpha.800}', _dark: '{colors.whiteAlpha.800}' } },
      },
    },
    drawer: {
      bg: {
        DEFAULT: { value: { _light: '{colors.white}', _dark: '{colors.gray.900}' } },
      },
    },
    select: {
      trigger: {
        outline: {
          fg: { value: { _light: '{colors.blackAlpha.800}', _dark: '{colors.whiteAlpha.800}' } },
        },
      },
      indicator: {
        fg: {
          DEFAULT: { value: { _light: '{colors.gray.400}', _dark: '{colors.gray.500}' } },
        },
      },
      placeholder: {
        fg: {
          DEFAULT: { value: { _light: '{colors.gray.400}', _dark: '{colors.gray.500}' } },
          error: { value: '{colors.red.500}' },
        },
      },
    },
    spinner: {
      track: {
        DEFAULT: { value: { _light: '{colors.blackAlpha.200}', _dark: '{colors.whiteAlpha.200}' } },
      },
    },
    badge: {
      gray: {
        bg: { value: { _light: '{colors.blackAlpha.50}', _dark: '{colors.whiteAlpha.100}' } },
        fg: { value: { _light: '{colors.blackAlpha.800}', _dark: '{colors.whiteAlpha.800}' } },
      },
      green: {
        bg: { value: { _light: '{colors.green.50}', _dark: '{colors.green.800}' } },
        fg: { value: { _light: '{colors.green.500}', _dark: '{colors.green.200}' } },
      },
      red: {
        bg: { value: { _light: '{colors.red.50}', _dark: '{colors.red.800}' } },
        fg: { value: { _light: '{colors.red.500}', _dark: '{colors.red.200}' } },
      },
      purple: {
        bg: { value: { _light: '{colors.purple.50}', _dark: '{colors.purple.800}' } },
        fg: { value: { _light: '{colors.purple.500}', _dark: '{colors.purple.100}' } },
      },
      purple_alt: {
        bg: { value: { _light: '{colors.purple.100}', _dark: '{colors.purple.800}' } },
        fg: { value: { _light: '{colors.blackAlpha.800}', _dark: '{colors.whiteAlpha.800}' } },
      },
      orange: {
        bg: { value: { _light: '{colors.orange.50}', _dark: '{colors.orange.800}' } },
        fg: { value: { _light: '{colors.orange.500}', _dark: '{colors.orange.100}' } },
      },
      blue: {
        bg: { value: { _light: '{colors.blue.50}', _dark: '{colors.blue.800}' } },
        fg: { value: { _light: '{colors.blue.500}', _dark: '{colors.blue.100}' } },
      },
      blue_alt: {
        bg: { value: { _light: '{colors.blue.50}', _dark: '{colors.blue.800}' } },
        fg: { value: { _light: '{colors.blackAlpha.800}', _dark: '{colors.whiteAlpha.800}' } },
      },
      yellow: {
        bg: { value: { _light: '{colors.yellow.50}', _dark: '{colors.yellow.800}' } },
        fg: { value: { _light: '{colors.yellow.500}', _dark: '{colors.yellow.100}' } },
      },
      teal: {
        bg: { value: { _light: '{colors.teal.50}', _dark: '{colors.teal.800}' } },
        fg: { value: { _light: '{colors.teal.500}', _dark: '{colors.teal.100}' } },
      },
      cyan: {
        bg: { value: { _light: '{colors.cyan.50}', _dark: '{colors.cyan.800}' } },
        fg: { value: { _light: '{colors.cyan.500}', _dark: '{colors.cyan.100}' } },
      },
      pink: {
        bg: { value: { _light: '{colors.pink.50}', _dark: '{colors.pink.800}' } },
        fg: { value: { _light: '{colors.pink.500}', _dark: '{colors.pink.100}' } },
      },
      // bright badges mainly used in other projects (e.g. autoscout, dev portal, etc.)
      bright: {
        gray: {
          bg: { value: { _light: '{colors.gray.100}', _dark: '{colors.gray.800}' } },
          fg: { value: { _light: '{colors.gray.600}', _dark: '{colors.gray.200}' } },
        },
        green: {
          bg: { value: { _light: '{colors.green.100}', _dark: '{colors.green.800}' } },
          fg: { value: { _light: '{colors.green.600}', _dark: '{colors.green.200}' } },
        },
        red: {
          bg: { value: { _light: '{colors.red.100}', _dark: '{colors.red.800}' } },
          fg: { value: { _light: '{colors.red.600}', _dark: '{colors.red.200}' } },
        },
        blue: {
          bg: { value: { _light: '{colors.blue.100}', _dark: '{colors.blue.800}' } },
          fg: { value: { _light: '{colors.blue.600}', _dark: '{colors.blue.200}' } },
        },
        yellow: {
          bg: { value: { _light: '{colors.yellow.100}', _dark: '{colors.yellow.800}' } },
          fg: { value: { _light: '{colors.yellow.600}', _dark: '{colors.yellow.200}' } },
        },
        teal: {
          bg: { value: { _light: '{colors.teal.100}', _dark: '{colors.teal.800}' } },
          fg: { value: { _light: '{colors.teal.600}', _dark: '{colors.teal.200}' } },
        },
        cyan: {
          bg: { value: { _light: '{colors.cyan.100}', _dark: '{colors.cyan.800}' } },
          fg: { value: { _light: '{colors.cyan.600}', _dark: '{colors.cyan.200}' } },
        },
        orange: {
          bg: { value: { _light: '{colors.orange.100}', _dark: '{colors.orange.600}' } },
          fg: { value: { _light: '{colors.orange.600}', _dark: '{colors.orange.100}' } },
        },
        purple: {
          bg: { value: { _light: '{colors.purple.50}', _dark: '{colors.purple.600}' } },
          fg: { value: { _light: '{colors.purple.600}', _dark: '{colors.purple.50}' } },
        },
        pink: {
          bg: { value: { _light: '{colors.pink.50}', _dark: '{colors.pink.600}' } },
          fg: { value: { _light: '{colors.pink.600}', _dark: '{colors.pink.50}' } },
        },
      },
    },
    tag: {
      root: {
        subtle: {
          bg: { value: { _light: '{colors.blackAlpha.50}', _dark: '{colors.whiteAlpha.100}' } },
          fg: { value: { _light: '{colors.blackAlpha.800}', _dark: '{colors.whiteAlpha.800}' } },
        },
        clickable: {
          bg: { value: { _light: '{colors.gray.100}', _dark: '{colors.gray.800}' } },
          fg: { value: { _light: '{colors.blackAlpha.800}', _dark: '{colors.whiteAlpha.800}' } },
        },
        filter: {
          bg: { value: { _light: '{colors.blue.50}', _dark: '{colors.whiteAlpha.200}' } },
        },
        select: {
          bg: {
            DEFAULT: { value: { _light: '{colors.gray.100}', _dark: '{colors.gray.800}' } },
          },
          fg: { value: { _light: '{colors.gray.500}', _dark: '{colors.whiteAlpha.800}' } },
        },
      },
    },
    table: {
      header: {
        bg: { value: { _light: '{colors.blackAlpha.100}', _dark: '{colors.whiteAlpha.200}' } },
        fg: { value: { _light: '{colors.blackAlpha.700}', _dark: '{colors.whiteAlpha.700}' } },
      },
    },
    checkbox: {
      control: {
        border: {
          DEFAULT: { value: { _light: '{colors.gray.100}', _dark: '{colors.gray.700}' } },
          hover: { value: { _light: '{colors.gray.200}', _dark: '{colors.gray.500}' } },
          readOnly: { value: { _light: '{colors.gray.200}', _dark: '{colors.gray.800}' } },
        },
      },
    },
    radio: {
      control: {
        border: {
          DEFAULT: { value: { _light: '{colors.gray.100}', _dark: '{colors.gray.700}' } },
          hover: { value: { _light: '{colors.gray.200}', _dark: '{colors.gray.500}' } },
          readOnly: { value: { _light: '{colors.gray.200}', _dark: '{colors.gray.800}' } },
        },
      },
    },
    stat: {
      indicator: {
        up: { value: { _light: '{colors.green.500}', _dark: '{colors.green.400}' } },
        down: { value: { _light: '{colors.red.600}', _dark: '{colors.red.400}' } },
      },
    },
    rating: {
      DEFAULT: { value: { _light: '{colors.gray.200}', _dark: '{colors.gray.700}' } },
      highlighted: { value: '{colors.yellow.400}' },
    },
  },
  shadows: {
    popover: {
      DEFAULT: { value: { _light: '{shadows.size.2xl}', _dark: '{shadows.dark-lg}' } },
    },
    drawer: {
      DEFAULT: { value: { _light: '{shadows.size.lg}', _dark: '{shadows.dark-lg}' } },
    },
  },
  opacity: {
    control: {
      disabled: { value: '0.2' },
    },
  },
};

export default semanticTokens;
