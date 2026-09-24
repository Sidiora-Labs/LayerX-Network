import { Text, Box, Flex } from '@chakra-ui/react';
import { sumBy } from 'es-toolkit';
import React from 'react';

import type { FormattedData } from './types';

import { getTokenTypeName } from 'lib/token/tokenTypes';
import { Link } from 'toolkit/chakra/link';
import { FilterInput } from 'toolkit/components/filters/FilterInput';
import { thinsp } from 'toolkit/utils/htmlEntities';
import IconSvg from 'ui/shared/IconSvg';

import type { Sort } from '../utils/tokenUtils';
import { getSortingFn, sortTokenGroups } from '../utils/tokenUtils';
import TokenSelectItem from './TokenSelectItem';

interface Props {
  searchTerm: string;
  getSort: (typeId: string) => Sort;
  filteredData: FormattedData;
  onInputChange: (searchTerm: string) => void;
  onSortClick: (event: React.SyntheticEvent) => void;
}

const TokenSelectMenu = ({ getSort, filteredData, onInputChange, onSortClick, searchTerm }: Props) => {
  const hasFilteredResult = sumBy(Object.values(filteredData), ({ items }) => items.length) > 0;

  return (
    <>
      <FilterInput
        placeholder="Search by token name"
        size="sm"
        inputProps={{ bgColor: 'dialog.bg' }}
        mb={ 5 }
        onChange={ onInputChange }
      />
      <Flex flexDir="column" rowGap={ 6 }>
        { Object.entries(filteredData).sort(sortTokenGroups).map(([ tokenType, tokenInfo ]) => {
          if (tokenInfo.items.length === 0) {
            return null;
          }

          const type = tokenType;
          const sortDirection = getSort(type);
          const arrowTransform = sortDirection === 'desc' ? 'rotate(90deg)' : 'rotate(-90deg)';
          const hasSort = tokenInfo.items.length > 1 && (
            (type === 'ERC-404' && tokenInfo.items.some(item => item.value)) ||
            type === 'ERC-1155' ||
            (type !== 'ERC-721' && tokenInfo.items.some(({ usd }) => usd))
          );
          const numPrefix = tokenInfo.isOverflow ? `>${ thinsp }` : '';

          return (
            <Box key={ type }>
              <Flex justifyContent="space-between">
                <Text mb={ 3 } color="gray.500" fontWeight={ 600 } fontSize="sm">
                  { getTokenTypeName(type) } tokens ({ numPrefix }{ tokenInfo.items.length })
                </Text>
                { hasSort && (
                  <Link data-type={ type } onClick={ onSortClick } aria-label={ `Sort ${ getTokenTypeName(type) } tokens` }>
                    <IconSvg name="arrows/east" boxSize={ 5 } transform={ arrowTransform } transitionDuration="faster"/>
                  </Link>
                ) }
              </Flex>
              { tokenInfo.items.sort(getSortingFn(type)(sortDirection)).map((data) =>
                <TokenSelectItem key={ data.token.address_hash + data.token_id } data={ data }/>) }
            </Box>
          );
        }) }
      </Flex>
      { Boolean(searchTerm) && !hasFilteredResult && <Text fontSize="sm">Could not find any matches.</Text> }
    </>
  );
};

export default React.memo(TokenSelectMenu);
