import { Table as ChakraTable, Icon } from '@chakra-ui/react';
import { throttle } from 'es-toolkit';
import * as React from 'react';

import ArrowIcon from 'icons/arrows/east.svg';

import { Link } from './link';

export const TableRoot = ChakraTable.Root;
export const TableBody = ChakraTable.Body;
export const TableHeader = ChakraTable.Header;
export const TableRow = ChakraTable.Row;

export interface TableCellProps extends ChakraTable.CellProps {
  isNumeric?: boolean;
}

export const TableCell = (props: TableCellProps) => {
  const { isNumeric, ...rest } = props;

  return <ChakraTable.Cell textAlign={ isNumeric ? 'right' : undefined } { ...rest }/>;
};

export interface TableColumnHeaderProps extends ChakraTable.ColumnHeaderProps {
  isNumeric?: boolean;
}

export const TableColumnHeader = (props: TableColumnHeaderProps) => {
  const { isNumeric, ...rest } = props;

  return <ChakraTable.ColumnHeader textAlign={ isNumeric ? 'right' : undefined } { ...rest }/>;
};

export interface TableColumnHeaderSortableProps<F extends string> extends TableColumnHeaderProps {
  sortField: F;
  sortValue: string;
  onSortToggle: (sortField: F) => void;
  disabled?: boolean;
  indicatorPosition?: 'left' | 'right';
  contentAfter?: React.ReactNode;
}

export const TableColumnHeaderSortable = <F extends string>(props: TableColumnHeaderSortableProps<F>) => {
  const { sortField, sortValue, onSortToggle, children, disabled, indicatorPosition = 'left', contentAfter, ...rest } = props;

  const handleSortToggle = React.useCallback(() => {
    onSortToggle(sortField);
  }, [ onSortToggle, sortField ]);

  return (
    <TableColumnHeader { ...rest }>
      <Link onClick={ disabled ? undefined : handleSortToggle } position="relative">
        { sortValue.includes(sortField) && (
          <Icon
            w={ 4 }
            h="100%"
            transform={ sortValue.toLowerCase().includes('asc') ? 'rotate(-90deg)' : 'rotate(90deg)' }
            position="absolute"
            left={ indicatorPosition === 'left' ? -5 : undefined }
            right={ indicatorPosition === 'right' ? -5 : undefined }
            top={ 0 }
          >
            <ArrowIcon/>
          </Icon>
        ) }
        { children }
      </Link>
      { contentAfter }
    </TableColumnHeader>
  );
};

export interface TableHeaderProps extends ChakraTable.HeaderProps {
  top?: number;
}

export const TableHeaderSticky = (props: TableHeaderProps) => {
  const { top, children, ...rest } = props;

  const ref = React.useRef<HTMLTableSectionElement>(null);
  const [ isStuck, setIsStuck ] = React.useState(false);

  const handleScroll = React.useCallback(() => {
    if (Number(ref.current?.getBoundingClientRect().y) <= (top || 0)) {
      setIsStuck(true);
    } else {
      setIsStuck(false);
    }
  }, [ top ]);

  React.useEffect(() => {
    const throttledHandleScroll = throttle(handleScroll, 300);

    window.addEventListener('scroll', throttledHandleScroll);

    return () => {
      window.removeEventListener('scroll', throttledHandleScroll);
    };
  }, [ handleScroll ]);

  return (
    <TableHeader
      ref={ ref }
      position="sticky"
      top={ top ? `${ top }px` : 0 }
      backgroundColor={{ _light: 'white', _dark: 'black' }}
      boxShadow={ isStuck ? 'action_bar' : 'none' }
      zIndex="1"
      { ...rest }
    >
      { children }
    </TableHeader>
  );
};
