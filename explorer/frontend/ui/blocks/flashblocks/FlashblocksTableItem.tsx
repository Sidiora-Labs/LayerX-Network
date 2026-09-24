import { HStack, Text } from '@chakra-ui/react';
import React from 'react';

import type { FlashblockItem } from 'types/client/flashblocks';

import { route } from 'nextjs-routes';

import { Link } from 'toolkit/chakra/link';
import { TableCell, TableRow } from 'toolkit/chakra/table';
import FlashblockEntity from 'ui/shared/entities/flashblock/FlashblockEntity';
import Time from 'ui/shared/time/Time';

interface Props {
  data: FlashblockItem;
}

const FlashblocksTableItem = ({ data }: Props) => {
  return (
    <TableRow>
      <TableCell>
        <HStack alignItems="center" gap={ 3 }>
          { data.block_number ? (
            <FlashblockEntity
              number={ data.block_number }
              index={ data.index }
              noIcon
            />
          ) : (
            <Text color="text.secondary">N/A</Text>
          ) }
          { data.timestamp && <Time color="text.secondary" timestamp={ data.timestamp } format="DD MMM, HH:mm:ss.SSS"/> }
        </HStack>
      </TableCell>
      <TableCell isNumeric>
        { data.transactions_count > 0 ? (
          <Link href={ route({
            pathname: '/block/[height_or_hash]',
            query: { height_or_hash: String(data.block_number), tab: 'txs' },
          }) }>
            { data.transactions_count }
          </Link>
        ) : (
          <Text color="text.secondary">{ data.transactions_count }</Text>
        ) }
      </TableCell>
      <TableCell isNumeric>
        { data.gas_used.toLocaleString() }
      </TableCell>
    </TableRow>
  );
};

export default React.memo(FlashblocksTableItem);
