import type { AccordionItemProps } from '@chakra-ui/react';
import { Box, chakra } from '@chakra-ui/react';
import React from 'react';

import type { FileTree } from './types';

import { AccordionItem, AccordionItemContent, AccordionItemTrigger, AccordionRoot } from 'toolkit/chakra/accordion';
import IconSvg from 'ui/shared/IconSvg';

import CodeEditorFileIcon from './CodeEditorFileIcon';
import CodeEditorMainFileIndicator from './CodeEditorMainFileIndicator';
import useThemeColors from './utils/useThemeColors';

interface Props {
  tree: FileTree;
  level?: number;
  isCollapsed?: boolean;
  onItemClick: (event: React.MouseEvent) => void;
  selectedFile: string;
  mainFile?: string;
}

const CodeEditorFileTree = ({ tree, level = 0, onItemClick, isCollapsed, selectedFile, mainFile }: Props) => {
  const [ value, setValue ] = React.useState<Array<string>>(isCollapsed ? [] : tree.map((item) => item.name));

  const handleValueChange = React.useCallback(({ value }: { value: Array<string> }) => {
    setValue(value);
  }, []);

  const itemProps: Partial<AccordionItemProps> = {
    borderWidth: '0px',
    cursor: 'pointer',
    lineHeight: '22px',
    _last: {
      borderBottomWidth: '0px',
    },
  };
  const themeColors = useThemeColors();

  return (
    <AccordionRoot multiple value={ value } onValueChange={ handleValueChange } noAnimation>
      {
        tree.map((leaf, index) => {
          const leafName = <chakra.span overflow="hidden" textOverflow="ellipsis" whiteSpace="nowrap">{ leaf.name }</chakra.span>;
          const isExpanded = value.includes(leaf.name);

          if ('children' in leaf) {
            return (
              <AccordionItem key={ index } value={ leaf.name } { ...itemProps }>
                <AccordionItemTrigger
                  pr="8px"
                  py="0"
                  pl={ `${ 8 + 8 * level }px` }
                  _hover={{ bgColor: themeColors['custom.list.hoverBackground'] }}
                  fontSize="13px"
                  lineHeight="22px"
                  h="22px"
                  transitionDuration="0"
                  noIndicator
                >
                  <Box
                    className="codicon codicon-tree-item-expanded"
                    transform="rotate(-90deg)"
                    _groupExpanded={{
                      transform: 'rotate(0deg)',
                    }}
                    boxSize="16px"
                    mr="2px"
                  />
                  <IconSvg
                    name={ isExpanded ? 'monaco/folder-open' : 'monaco/folder' }
                    boxSize="16px"
                    mr="4px"
                  />
                  { leafName }
                </AccordionItemTrigger>
                <AccordionItemContent p="0">
                  <CodeEditorFileTree
                    tree={ leaf.children }
                    level={ level + 1 }
                    onItemClick={ onItemClick }
                    isCollapsed={ isCollapsed }
                    selectedFile={ selectedFile }
                    mainFile={ mainFile }
                  />
                </AccordionItemContent>
              </AccordionItem>
            );
          }

          return (
            <AccordionItem
              key={ index }
              value={ leaf.name }
              { ...itemProps }
              pl={ `${ 26 + (level * 8) }px` }
              pr="8px"
              onClick={ onItemClick }
              data-file-path={ leaf.file_path }
              display="flex"
              position="relative"
              alignItems="center"
              overflow="hidden"
              _hover={{
                bgColor: selectedFile === leaf.file_path ? themeColors['list.inactiveSelectionBackground'] : themeColors['custom.list.hoverBackground'],
              }}
              bgColor={ selectedFile === leaf.file_path ? themeColors['list.inactiveSelectionBackground'] : 'none' }
            >
              { mainFile === leaf.file_path && (
                <CodeEditorMainFileIndicator
                  position="absolute"
                  top={ `${ (22 - 12) / 2 }px` }
                  left={ `${ (26 - 12 - 2) + (level * 8) }px` }
                />
              ) }
              <CodeEditorFileIcon fileName={ leaf.name } mr="4px"/>
              { leafName }
            </AccordionItem>
          );
        })
      }
    </AccordionRoot>
  );
};

export default React.memo(CodeEditorFileTree);
