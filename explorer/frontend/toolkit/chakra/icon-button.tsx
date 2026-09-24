import React from 'react';

import { Button, type ButtonProps } from './button';

export interface IconButtonProps extends Omit<ButtonProps, 'size'> {
  size?: '2xs' | '2xs_alt' | 'md';
}

export const IconButton = React.forwardRef<HTMLButtonElement, IconButtonProps>(
  function IconButton(props, ref) {
    const { size, variant = 'plain', children, ...rest } = props;

    // FIXME: I have to clone the children instead of using _icon props because of style overrides
    // in some pw tests for some reason the _icon style will be applied before the style of child (IconSvg component)
    const child = React.Children.only<React.ReactElement>(children as React.ReactElement);
    const clonedChildren = size ? React.cloneElement(child, { boxSize: size === '2xs_alt' ? 3 : 5 } as React.HTMLAttributes<HTMLElement>) : child;

    const sizeStyle = (() => {
      switch (size) {
        case '2xs': {
          return {
            _icon: { boxSize: 5 },
            boxSize: 5,
            borderRadius: 'sm',
          };
        }
        case '2xs_alt': {
          return {
            _icon: { boxSize: 3 },
            boxSize: 5,
            borderRadius: 'sm',
          };
        }
        case 'md': {
          return {
            _icon: { boxSize: 5 },
            boxSize: 8,
          };
        }
        default:
          return {};
      }
    })();

    return (
      <Button
        ref={ ref }
        display="inline-flex"
        justifyContent="center"
        alignItems="center"
        p={ 0 }
        minW="auto"
        flexShrink="0"
        variant={ variant }
        { ...sizeStyle }
        { ...rest }
      >
        { clonedChildren }
      </Button>
    );
  },
);
