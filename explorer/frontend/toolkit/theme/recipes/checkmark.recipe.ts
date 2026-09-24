import { defineRecipe } from '@chakra-ui/react';

export const recipe = defineRecipe({
  className: 'chakra-checkmark',
  base: {
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    flexShrink: '0',
    color: 'white',
    borderWidth: '2px',
    borderColor: 'transparent',
    focusVisibleRing: 'outside',
    _icon: {
      boxSize: 'full',
    },
    _disabled: {
      opacity: 'control.disabled',
    },
  },
  variants: {
    size: {
      xs: {
        boxSize: '3',
        borderRadius: '2px',
      },
      sm: {
        boxSize: '4',
        borderRadius: '2px',
      },
      md: {
        boxSize: '5',
        borderRadius: 'sm',
      },
    },

    variant: {
      solid: {
        borderColor: 'checkbox.control.border',
        _hover: {
          borderColor: 'checkbox.control.border.hover',
        },
        _readOnly: {
          borderColor: 'checkbox.control.border.readOnly',
          _hover: {
            borderColor: 'checkbox.control.border.readOnly',
          },
          '&:is([data-state=checked], [data-state=indeterminate])': {
            bg: 'checkbox.control.border.readOnly',
            color: 'gray.500',
            _hover: {
              bg: 'checkbox.control.border.readOnly',
            },
          },
        },
        '&:is([data-state=checked], [data-state=indeterminate])': {
          bg: 'selected.option.bg',
          color: 'white',
          borderColor: 'selected.option.bg',
          _hover: {
            bg: 'hover',
            borderColor: 'hover',
          },
        },
      },
    },
  },

  defaultVariants: {
    variant: 'solid',
    size: 'md',
  },
});
