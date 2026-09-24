import { Box } from '@chakra-ui/react';
import React from 'react';

import type { File } from './types';

import CodeEditorFileTree from './CodeEditorFileTree';
import CoderEditorCollapseButton from './CoderEditorCollapseButton';
import composeFileTree from './utils/composeFileTree';

interface Props {
  data: Array<File>;
  onFileSelect: (index: number) => void;
  selectedFile: string;
  mainFile?: string;
  isActive: boolean;
  setActionBarRenderer: React.Dispatch<React.SetStateAction<(() => React.JSX.Element) | undefined>>;
}

const CodeEditorFileExplorer = ({ data, onFileSelect, selectedFile, mainFile, isActive, setActionBarRenderer }: Props) => {
  const [ key, setKey ] = React.useState(0);
  const tree = React.useMemo(() => {
    return composeFileTree(data);
  }, [ data ]);

  const handleCollapseButtonClick = React.useCallback(() => {
    setKey((prev) => prev + 1);
  }, []);

  const renderActionBar = React.useCallback(() => {
    return (
      <CoderEditorCollapseButton onClick={ handleCollapseButtonClick } label="Collapse folders"/>
    );
  }, [ handleCollapseButtonClick ]);

  const handleFileClick = React.useCallback((event: React.MouseEvent) => {
    const filePath = (event.currentTarget as HTMLDivElement).getAttribute('data-file-path');
    const fileIndex = data.findIndex((item) => item.file_path === filePath);

    if (fileIndex > -1) {
      onFileSelect(fileIndex);
    }
  }, [ data, onFileSelect ]);

  React.useEffect(() => {
    isActive && setActionBarRenderer(() => renderActionBar);
  }, [ isActive, renderActionBar, setActionBarRenderer ]);

  return (
    <Box>
      <CodeEditorFileTree
        key={ key }
        tree={ tree }
        onItemClick={ handleFileClick }
        isCollapsed={ key > 0 }
        selectedFile={ selectedFile }
        mainFile={ mainFile }
      />
    </Box>
  );
};

export default React.memo(CodeEditorFileExplorer);
