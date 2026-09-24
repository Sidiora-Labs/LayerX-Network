import { Box, Flex } from '@chakra-ui/react';
import { useRouter } from 'next/router';
import React from 'react';

import type { AddressMudRecords, AddressMudRecordsFilter, AddressMudRecordsSorting } from 'types/api/address';

import { route } from 'nextjs-routes';

import capitalizeFirstLetter from 'lib/capitalizeFirstLetter';
import useIsMobile from 'lib/hooks/useIsMobile';
import { Link } from 'toolkit/chakra/link';
import { TableBody, TableCell, TableColumnHeader, TableHeaderSticky, TableRoot, TableRow } from 'toolkit/chakra/table';
import type { TableColumnHeaderProps } from 'toolkit/chakra/table';
import { Tooltip } from 'toolkit/chakra/tooltip';
import { middot } from 'toolkit/utils/htmlEntities';
import CopyToClipboard from 'ui/shared/CopyToClipboard';
import IconSvg from 'ui/shared/IconSvg';
import Time from 'ui/shared/time/Time';

import AddressMudRecordsKeyFilter from './AddressMudRecordsKeyFilter';
import { getNameTypeText, getValueString } from './utils';

const COL_MIN_WIDTH = 180;
const COL_MIN_WIDTH_MOBILE = 140;
const CUT_COL_WIDTH = 36;
const MIN_CUT_COUNT = 2;

type Props = {
  data: AddressMudRecords;
  top: number;
  sorting?: AddressMudRecordsSorting;
  toggleSorting: (key: AddressMudRecordsSorting['sort']) => void;
  setFilters: React.Dispatch<React.SetStateAction<AddressMudRecordsFilter>>;
  filters: AddressMudRecordsFilter;
  toggleTableHasHorizontalScroll: () => void;
  scrollRef?: React.RefObject<HTMLDivElement>;
  hash: string;
};

const AddressMudRecordsTable = ({
  data,
  top,
  sorting,
  toggleSorting,
  filters,
  setFilters,
  toggleTableHasHorizontalScroll,
  scrollRef,
  hash,
}: Props) => {
  const totalColsCut = data.schema.key_names.length + data.schema.value_names.length;
  const isMobile = useIsMobile();
  const [ colsCutCount, setColsCutCount ] = React.useState<number>(isMobile ? MIN_CUT_COUNT : 0);
  const [ isOpened, setIsOpened ] = React.useState(false);
  const [ hasCut, setHasCut ] = React.useState(isMobile ? totalColsCut > MIN_CUT_COUNT : true);

  const containerRef = React.useRef<HTMLTableElement>(null);
  const tableRef = React.useRef<HTMLTableElement>(null);

  const router = useRouter();

  const toggleIsOpen = React.useCallback(() => {
    isOpened && tableRef.current?.scroll({ left: 0 });
    setIsOpened((prev) => !prev);
    toggleTableHasHorizontalScroll();
  }, [ setIsOpened, toggleTableHasHorizontalScroll, isOpened ]);

  const onRecordClick = React.useCallback((e: React.MouseEvent) => {
    if (e.metaKey || e.ctrlKey) {
      // Allow opening in a new tab/window with right-click or ctrl/cmd+click
      return;
    }

    e.preventDefault();

    const recordId = e.currentTarget.getAttribute('data-id');
    if (recordId) {
      router.push(
        { pathname: '/address/[hash]', query: { hash, tab: 'mud', table_id: data.table.table_id, record_id: recordId } },
        undefined,
        { shallow: true },
      );
    }
    scrollRef?.current?.scrollIntoView();
  }, [ router, scrollRef, hash, data.table.table_id ]);

  const handleFilterChange = React.useCallback((field: keyof AddressMudRecordsFilter) => (val: string) => {
    setFilters(prev => {
      const newVal = { ...prev };
      newVal[field] = val;
      return newVal;
    });
  }, [ setFilters ]);

  const onKeySortClick = React.useCallback(
    (e: React.MouseEvent) => toggleSorting('key' + e.currentTarget.getAttribute('data-id') as AddressMudRecordsSorting['sort']),
    [ toggleSorting ],
  );

  React.useEffect(() => {
    if (hasCut && !colsCutCount && containerRef.current) {
      const count = Math.floor((containerRef.current.getBoundingClientRect().width - CUT_COL_WIDTH) / COL_MIN_WIDTH);
      if (totalColsCut > MIN_CUT_COUNT && count - 1 < totalColsCut) {
        setColsCutCount(count - 1);
      } else {
        setHasCut(false);
      }
    }
  }, [ colsCutCount, data.schema, hasCut, setHasCut, totalColsCut ]);

  const colW = isMobile ? COL_MIN_WIDTH_MOBILE : COL_MIN_WIDTH;

  const keys = (isOpened || !hasCut) ? data.schema.key_names : data.schema.key_names.slice(0, colsCutCount);
  const values = (isOpened || !hasCut) ? data.schema.value_names : data.schema.value_names.slice(0, colsCutCount - data.schema.key_names.length);
  const colsCount = (isOpened || !hasCut) ? totalColsCut : colsCutCount;

  const tdStyles: TableColumnHeaderProps = {
    wordBreak: 'break-word',
    whiteSpace: 'normal',
    minW: `${ colW }px`,
    w: `${ 100 / colsCount }%`,
    verticalAlign: 'top',
    lineHeight: '20px',
  };

  const hasHorizontalScroll = isMobile || isOpened;

  if (hasCut && !colsCutCount) {
    return <Box w="100%" ref={ containerRef }></Box>;
  }

  const cutButton = (
    <TableColumnHeader width={ `${ CUT_COL_WIDTH }px ` } verticalAlign="baseline">
      <Tooltip content={ isOpened ? 'Hide columns' : 'Show all columns' }>
        <Link onClick={ toggleIsOpen } aria-label="show/hide columns">{ middot }{ middot }{ middot }</Link>
      </Tooltip>
    </TableColumnHeader>
  );

  return (
    // can't implement both horizontal table scroll and sticky header
    <Box maxW="100%" overflowX={ hasHorizontalScroll ? 'scroll' : 'unset' } whiteSpace="nowrap" ref={ tableRef }>
      <TableRoot style={{ tableLayout: 'fixed' }}>
        <TableHeaderSticky top={ hasHorizontalScroll ? 0 : top } display={ hasHorizontalScroll ? 'table' : 'table-header-group' } w="100%">
          <TableRow>
            { keys.map((keyName, index) => {
              const text = getNameTypeText(keyName, data.schema.key_types[index]);
              return (
                <TableColumnHeader key={ keyName } { ...tdStyles }>
                  { index < 2 ? (
                    <Flex alignItems="center">
                      <Link
                        onClick={ onKeySortClick }
                        data-id={ index }
                        display="flex"
                        alignItems="start"
                        lineHeight="20px"
                        mr={ 2 }
                      >
                        { sorting?.sort === `key${ index }` && sorting.order && (
                          <Box minW="24px" w="24px" mr={ 2 }>
                            <IconSvg
                              name="arrows/east"
                              boxSize={ 5 }
                              transform={ sorting.order === 'asc' ? 'rotate(-90deg)' : 'rotate(90deg)' }
                            />
                          </Box>
                        ) }
                        { text }
                      </Link>
                      <Box minW="20px" w="20px">
                        <AddressMudRecordsKeyFilter
                          value={ filters[index === 0 ? 'filter_key0' : 'filter_key1'] }
                          title={ text }
                          columnName={ keyName }
                          handleFilterChange={ handleFilterChange(index === 0 ? 'filter_key0' : 'filter_key1') }
                        />
                      </Box>
                    </Flex>
                  ) : text }
                </TableColumnHeader>
              );
            }) }
            { values.map((valName, index) => (
              <TableColumnHeader key={ valName } { ...tdStyles }>
                { capitalizeFirstLetter(valName) } ({ data.schema.value_types[index] })
              </TableColumnHeader>
            )) }
            { hasCut && !isOpened && cutButton }
            <TableColumnHeader { ...tdStyles } w={ `${ colW }px` }>Modified</TableColumnHeader>
            { hasCut && isOpened && cutButton }
          </TableRow>
        </TableHeaderSticky>
        <TableBody display={ hasHorizontalScroll ? 'table' : 'table-row-group' } w="100%">
          { data.items.map((item) => (
            <TableRow key={ item.id }>
              { keys.map((keyName, index) => (
                <TableCell key={ keyName } backgroundColor={{ _light: 'blackAlpha.50', _dark: 'whiteAlpha.50' }} { ...tdStyles }>
                  { index === 0 ? (
                    <Link
                      onClick={ onRecordClick }
                      data-id={ item.id }
                      fontWeight={ 700 }
                      href={ route({ pathname: '/address/[hash]', query: { hash, tab: 'mud', table_id: data.table.table_id, record_id: item.id } }) }
                      display="inline"
                    >
                      { getValueString(item.decoded[keyName]) }
                    </Link>
                  ) : getValueString(item.decoded[keyName]) }
                  <CopyToClipboard text={ String(item.decoded[keyName]) }/>
                </TableCell>
              )) }
              { values.map((valName) =>
                <TableCell key={ valName } { ...tdStyles }>{ getValueString(item.decoded[valName]) }</TableCell>) }
              { hasCut && !isOpened && <TableCell width={ `${ CUT_COL_WIDTH }px ` }></TableCell> }
              <TableCell { ...tdStyles } color="text.secondary" w={ `${ colW }px` }><Time timestamp={ item.timestamp }/></TableCell>
              { hasCut && isOpened && <TableCell width={ `${ CUT_COL_WIDTH }px ` }></TableCell> }
            </TableRow>
          )) }
        </TableBody>
      </TableRoot>
    </Box>
  );
};

export default AddressMudRecordsTable;
