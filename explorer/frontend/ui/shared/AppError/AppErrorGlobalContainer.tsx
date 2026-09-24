import { Box } from '@chakra-ui/react';
import React from 'react';

interface Props {
  children: React.ReactNode;
}

const AppErrorGlobalContainer = ({ children }: Props) => {
  return <Box bgColor="bg.primary">{ children }</Box>;
};

export default React.memo(AppErrorGlobalContainer);
