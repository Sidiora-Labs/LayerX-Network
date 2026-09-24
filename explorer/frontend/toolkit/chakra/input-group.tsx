import type { BoxProps, InputElementProps } from '@chakra-ui/react';
import { Group, InputElement } from '@chakra-ui/react';
import { debounce } from 'es-toolkit';
import * as React from 'react';

import getComponentDisplayName from '../utils/getComponentDisplayName';
import type { InputProps } from './input';

export interface InputGroupProps extends BoxProps {
  startElementProps?: InputElementProps;
  endElementProps?: InputElementProps;
  startElement?: React.ReactNode;
  endElement?: React.ReactNode;
  children: React.ReactElement<InputElementProps>;
  startOffset?: InputElementProps['paddingStart'];
  endOffset?: InputElementProps['paddingEnd'];
}

export const InputGroup = React.forwardRef<HTMLDivElement, InputGroupProps>(
  function InputGroup(props, ref) {
    const {
      startElement,
      startElementProps,
      endElement,
      endElementProps,
      children,
      startOffset,
      endOffset,
      ...rest
    } = props;

    const startElementRef = React.useRef<HTMLDivElement>(null);
    const endElementRef = React.useRef<HTMLDivElement>(null);
    const groupRef = React.useRef<HTMLDivElement>(null);

    const [ inlinePaddings, setInlinePaddings ] = React.useState<{ start?: number; end?: number }>();

    const calculateInlinePaddings = React.useCallback(() => {
      const startWidth = startElementRef.current?.getBoundingClientRect().width ?? 0;
      const endWidth = endElementRef.current?.getBoundingClientRect().width ?? 0;

      setInlinePaddings({
        start: startWidth,
        end: endWidth,
      });
    }, []);

    React.useEffect(() => {
      if (!groupRef.current) return;

      let timeoutId: ReturnType<typeof setTimeout>;

      const intersectionObserver = new IntersectionObserver(
        (entries) => {
          const entry = entries[0];
          if (entry && entry.isIntersecting) {
            // Small delay to ensure rendering is complete
            timeoutId = setTimeout(calculateInlinePaddings, 50);
          }
        },
        { threshold: 0.01 },
      );

      intersectionObserver.observe(groupRef.current);

      return () => {
        intersectionObserver.disconnect();
        if (timeoutId) {
          clearTimeout(timeoutId);
        }
      };
    }, [ calculateInlinePaddings ]);

    React.useEffect(() => {
      calculateInlinePaddings();

      const resizeHandler = debounce(calculateInlinePaddings, 300);
      const resizeObserver = new ResizeObserver(resizeHandler);

      if (groupRef.current) {
        resizeObserver.observe(groupRef.current);
      }

      return function cleanup() {
        resizeObserver.disconnect();
      };
    }, [ calculateInlinePaddings ]);

    // Combine refs for the Group component
    const combinedRef = React.useCallback((node: HTMLDivElement) => {
      groupRef.current = node;
      if (typeof ref === 'function') {
        ref(node);
      } else if (ref) {
        ref.current = node;
      }
    }, [ ref ]);

    return (
      <Group ref={ combinedRef } w="100%" { ...rest }>
        { startElement && (
          <InputElement pointerEvents="none" ref={ startElementRef } px={ 0 } color="input.element" { ...startElementProps }>
            { startElement }
          </InputElement>
        ) }
        { React.Children.map(children, (child: React.ReactElement<InputProps>) => {
          if (getComponentDisplayName(child.type) !== 'FieldInput') {
            return child;
          }
          return React.cloneElement(child, {
            ...(startElement && { ps: startOffset ?? (inlinePaddings?.start ? `${ inlinePaddings.start }px` : undefined) }),
            ...(endElement && { pe: endOffset ?? (inlinePaddings?.end ? `${ inlinePaddings.end }px` : undefined) }),
            // hide input value and placeholder for the first render
            value: inlinePaddings ? child.props.value : undefined,
            placeholder: inlinePaddings ? child.props.placeholder : undefined,
          });
        }) }
        { endElement && (
          <InputElement placement="end" ref={ endElementRef } px={ 0 } color="input.element" { ...endElementProps }>
            { endElement }
          </InputElement>
        ) }
      </Group>
    );
  },
);

InputGroup.displayName = 'InputGroup';
