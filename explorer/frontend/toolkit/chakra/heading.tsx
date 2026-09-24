import type { HeadingProps as ChakraHeadingProps } from '@chakra-ui/react';
import { Heading as ChakraHeading } from '@chakra-ui/react';
import React from 'react';

export interface HeadingProps extends ChakraHeadingProps {
  level?: '1' | '2' | '3';
}

export const Heading = React.forwardRef<HTMLHeadingElement, HeadingProps>(
  function Heading(props, ref) {
    const { level, ...rest } = props;

    const textStyle = (() => {
      switch (level) {
        case '1':
          return { base: 'heading.md', lg: 'heading.xl' };
        case '2':
          return { base: 'heading.sm', lg: 'heading.lg' };
        case '3':
          return { base: 'heading.xs', lg: 'heading.md' };
      }
    })();

    const as = (() => {
      switch (level) {
        case '1':
          return 'h1';
        case '2':
          return 'h2';
        case '3':
          return 'h3';
      }
    })();

    return <ChakraHeading ref={ ref } color="heading" textStyle={ textStyle } as={ as } { ...rest }/>;
  });
