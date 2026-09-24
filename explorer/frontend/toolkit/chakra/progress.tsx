import { Progress as ChakraProgress } from '@chakra-ui/react';
import * as React from 'react';

interface ProgressProps extends ChakraProgress.RootProps {
  trackProps?: ChakraProgress.TrackProps;
}

export const Progress = React.forwardRef<HTMLDivElement, ProgressProps>(
  function Progress(props, ref) {
    const { trackProps, color, ...rest } = props;
    return (
      <ChakraProgress.Root { ...rest } ref={ ref }>
        <ChakraProgress.Track { ...trackProps }>
          <ChakraProgress.Range bg={ color }/>
        </ChakraProgress.Track>
      </ChakraProgress.Root>
    );
  },
);
