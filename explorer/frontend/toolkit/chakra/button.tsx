import type { ButtonProps as ChakraButtonProps, ButtonGroupProps as ChakraButtonGroupProps } from '@chakra-ui/react';
import {
  Button as ChakraButton,
  ButtonGroup as ChakraButtonGroup,
} from '@chakra-ui/react';
import * as React from 'react';

import { Skeleton } from './skeleton';

interface ButtonLoadingProps {
  loading?: boolean;
  loadingText?: React.ReactNode;
  loadingSkeleton?: boolean;
}

export interface ButtonProps extends ChakraButtonProps, ButtonLoadingProps {
  expanded?: boolean;
  selected?: boolean;
  highlighted?: boolean;
}

export const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  function Button(props, ref) {
    const { loading, disabled, children, expanded, selected, highlighted, loadingSkeleton = false, ...rest } = props;

    return (
      <Skeleton loading={ loadingSkeleton } asChild ref={ ref as React.ForwardedRef<HTMLDivElement> }>
        <ChakraButton
          { ...(expanded ? { 'data-expanded': true } : {}) }
          { ...(selected ? { 'data-selected': true } : {}) }
          { ...(highlighted ? { 'data-highlighted': true } : {}) }
          { ...(loadingSkeleton ? { 'data-loading-skeleton': true } : {}) }
          disabled={ !loadingSkeleton && (loading || disabled) }
          loading={ loading }
          { ...rest }
        >
          { children }
        </ChakraButton>
      </Skeleton>
    );
  },
);

export interface ButtonGroupProps extends ChakraButtonGroupProps {}

export const ButtonGroup = React.forwardRef<HTMLDivElement, ButtonGroupProps>(
  function ButtonGroup(props, ref) {
    const { ...rest } = props;

    return (
      <ChakraButtonGroup ref={ ref } { ...rest }/>
    );
  },
);

export interface ButtonGroupRadioProps extends Omit<ChakraButtonGroupProps, 'children' | 'onChange'> {
  children: Array<React.ReactElement<ButtonProps>>;
  onChange?: (value: string) => void;
  defaultValue?: string;
  loading?: boolean;
  equalWidth?: boolean;
}

export const ButtonGroupRadio = React.forwardRef<HTMLDivElement, ButtonGroupRadioProps>(
  function ButtonGroupRadio(props, ref) {
    const { children, onChange, variant = 'segmented', defaultValue, loading = false, equalWidth = false, ...rest } = props;

    const firstChildValue = React.useMemo(() => {
      const firstChild = Array.isArray(children) ? children[0] : undefined;
      return typeof firstChild?.props.value === 'string' ? firstChild.props.value : undefined;
    }, [ children ]);

    const [ value, setValue ] = React.useState<string | undefined>(defaultValue ?? firstChildValue);

    const handleItemClick = React.useCallback((event: React.MouseEvent<HTMLButtonElement>) => {
      const value = event.currentTarget.value;
      setValue(value);
      onChange?.(value);
    }, [ onChange ]);

    const clonedChildren = React.Children.map(children, (child: React.ReactElement<ButtonProps>) => {
      return React.cloneElement(child, {
        onClick: handleItemClick,
        selected: value === child.props.value,
        variant,
      });
    });

    const childrenLength = React.Children.count(children);

    return (
      <Skeleton loading={ loading }>
        <ChakraButtonGroup
          ref={ ref }
          gap={ 0 }
          { ...(equalWidth ? {
            display: 'grid',
            gridTemplateColumns: `repeat(${ childrenLength }, 1fr)`,
          } : {}) }
          { ...rest }
        >
          { clonedChildren }
        </ChakraButtonGroup>
      </Skeleton>
    );
  },
);
