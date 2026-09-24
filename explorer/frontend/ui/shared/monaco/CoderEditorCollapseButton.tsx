import { Box } from '@chakra-ui/react';
import React from 'react';

import useThemeColors from './utils/useThemeColors';

interface Props {
  onClick: () => void;
  label: string;
  isDisabled?: boolean;
  isCollapsed?: boolean;
}

const CoderEditorCollapseButton = ({ onClick, label, isDisabled, isCollapsed }: Props) => {
  const themeColors = useThemeColors();

  return (
    <Box
      ml="auto"
      alignSelf="center"
      className={ isCollapsed ? 'codicon codicon-search-expand-results' : 'codicon codicon-collapse-all' }
      opacity={ isDisabled ? 0.6 : 1 }
      boxSize="20px"
      p="2px"
      borderRadius="sm"
      _before={{
        content: isCollapsed ? '"\\eb95"' : '"\\eac5"',
      }}
      _hover={{
        bgColor: themeColors['custom.inputOption.hoverBackground'],
      }}
      onClick={ onClick }
      cursor="pointer"
      title={ label }
      aria-label={ label }
    />
  );
};

export default React.memo(CoderEditorCollapseButton);
