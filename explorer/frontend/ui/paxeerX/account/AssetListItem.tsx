import { Flex, Text } from '@chakra-ui/react';
import React from 'react';

import type { PaxeerXBalance } from 'types/api/paxeerX';

import { IconButton } from 'toolkit/chakra/icon-button';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { TableCell, TableRow } from 'toolkit/chakra/table';
import IconSvg from 'ui/shared/IconSvg';

import { assetLabel, formatAmount } from './utils';

const PART_LABELS: Array<{ key: keyof PaxeerXBalance['parts']; label: string }> = [
  { key: 'chain', label: 'On chain' },
  { key: 'custody', label: 'In custody' },
  { key: 'kernel', label: 'In kernel' },
];

export interface Props {
  item: PaxeerXBalance;
  isLoading?: boolean;
}

const AssetListItem = ({ item, isLoading }: Props) => {
  const [ isExpanded, setIsExpanded ] = React.useState(false);

  const handleToggle = React.useCallback(() => {
    setIsExpanded((prev) => !prev);
  }, []);

  const label = assetLabel(item.asset);

  return (
    <>
      <TableRow data-asset={ item.asset.id }>
        <TableCell verticalAlign="middle">
          <Flex columnGap={ 2 } alignItems="center">
            <IconButton
              aria-label={ isExpanded ? `Hide ${ label } breakdown` : `Show ${ label } breakdown` }
              aria-expanded={ isExpanded }
              onClick={ handleToggle }
              disabled={ isLoading }
              variant="icon_secondary"
              size="2xs"
              boxSize={ 5 }
            >
              <IconSvg name="arrows/east-mini" transform={ isExpanded ? 'rotate(90deg)' : undefined }/>
            </IconButton>
            <Skeleton loading={ isLoading } fontWeight={ 600 }>{ label }</Skeleton>
          </Flex>
        </TableCell>
        <TableCell verticalAlign="middle">
          <Skeleton loading={ isLoading } color="text.secondary" wordBreak="break-all">{ item.asset.denom }</Skeleton>
        </TableCell>
        <TableCell verticalAlign="middle" isNumeric>
          <Skeleton loading={ isLoading } display="inline-block" data-label="total">
            { formatAmount(item.total, item.asset) }
          </Skeleton>
        </TableCell>
      </TableRow>
      { isExpanded && (
        <TableRow data-parts-of={ item.asset.id }>
          <TableCell colSpan={ 3 } pt={ 0 }>
            <Flex flexDirection="column" rowGap={ 1 } pl={ 7 }>
              { PART_LABELS.map(({ key, label: partLabel }) => (
                <Flex key={ key } columnGap={ 2 } justifyContent="space-between" maxW="480px">
                  <Text color="text.secondary">{ partLabel }</Text>
                  <Text data-part={ key }>{ formatAmount(item.parts[key], item.asset) }</Text>
                </Flex>
              )) }
            </Flex>
          </TableCell>
        </TableRow>
      ) }
    </>
  );
};

export default React.memo(AssetListItem);
