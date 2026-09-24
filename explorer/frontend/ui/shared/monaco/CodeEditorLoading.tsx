import { Center, chakra } from '@chakra-ui/react';
import React from 'react';

import { ContentLoader } from 'toolkit/components/loaders/ContentLoader';

import useThemeColors from './utils/useThemeColors';

interface Props {
  className?: string;
}

const CodeEditorLoading = ({ className }: Props) => {
  const themeColors = useThemeColors();

  return (
    <Center bgColor={ themeColors['editor.background'] } w="100%" h="100%" overflow="hidden" className={ className }>
      <ContentLoader/>
    </Center>
  );
};

export default React.memo(chakra(CodeEditorLoading));
