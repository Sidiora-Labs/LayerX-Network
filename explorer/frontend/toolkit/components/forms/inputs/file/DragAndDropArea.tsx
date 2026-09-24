import { chakra, Center } from '@chakra-ui/react';
import type { DragEvent } from 'react';
import React from 'react';

import { getAllFileEntries, convertFileEntryToFile } from './utils';

interface Props {
  children: React.ReactNode;
  onDrop: (files: Array<File>) => void;
  className?: string;
  isDisabled?: boolean;
  isInvalid?: boolean;
  fullFilePath?: boolean;
}

export const DragAndDropArea = chakra(({ onDrop, children, className, isDisabled, fullFilePath, isInvalid }: Props) => {
  const [ isDragOver, setIsDragOver ] = React.useState(false);

  const handleDrop = React.useCallback(async(event: DragEvent<HTMLDivElement>) => {
    event.preventDefault();

    if (isDisabled) {
      return;
    }

    const fileEntries = await getAllFileEntries(event.dataTransfer.items);
    const files = await Promise.all(fileEntries.map((fileEntry) => convertFileEntryToFile(fileEntry, fullFilePath)));

    onDrop(files);
    setIsDragOver(false);
  }, [ isDisabled, onDrop, fullFilePath ]);

  const handleDragOver = React.useCallback((event: DragEvent<HTMLDivElement>) => {
    event.preventDefault();
  }, []);

  const handleDragEnter = React.useCallback((event: DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    setIsDragOver(true);
  }, []);

  const handleDragLeave = React.useCallback((event: DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    setIsDragOver(false);
  }, []);

  const handleClick = React.useCallback((event: React.MouseEvent) => {
    if (isDisabled) {
      event.preventDefault();
      event.stopPropagation();
    }
  }, [ isDisabled ]);

  return (
    <Center
      className={ className }
      w="100%"
      minH="120px"
      borderWidth="2px"
      borderRadius="base"
      borderStyle="dashed"
      borderColor={ isDragOver ? 'input.border.hover' : 'input.border' }
      cursor="pointer"
      textAlign="center"
      { ...(isDisabled ? { 'data-disabled': true } : {}) }
      { ...(isInvalid ? { 'data-invalid': true } : {}) }
      color="input.placeholder"
      _disabled={{ opacity: 'control.disabled' }}
      _invalid={{ borderColor: 'input.border.error', color: 'input.placeholder.error' }}
      _hover={{ borderColor: 'input.border.hover' }}
      onClick={ handleClick }
      onDrop={ handleDrop }
      onDragOver={ handleDragOver }
      onDragEnter={ handleDragEnter }
      onDragLeave={ handleDragLeave }
    >
      { children }
    </Center>
  );
});
