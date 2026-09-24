import { Flex, Text } from '@chakra-ui/react';
import React from 'react';

import type { FlashblockItem } from 'types/client/flashblocks';

import { route } from 'nextjs-routes';

import { Link } from 'toolkit/chakra/link';
import FlashblockEntity from 'ui/shared/entities/flashblock/FlashblockEntity';
import ListItemMobile from 'ui/shared/ListItemMobile/ListItemMobile';
import Time from 'ui/shared/time/Time';

interface Props {
  data: FlashblockItem;
}

const FlashblocksListItem = ({ data }: Props) => {
  return (
    <ListItemMobile rowGap={ 3 }>
      <Flex justifyContent="space-between" w="100%">
        { data.block_number ? (
          <FlashblockEntity
            number={ data.block_number }
            index={ data.index }
            fontWeight={ 600 }
          />
        ) : <Text color="text.secondary">N/A</Text> }
      </Flex>
      { data.timestamp && (
        <Flex columnGap={ 2 }>
          <Text fontWeight={ 500 }>Timestamp</Text>
          <Time color="text.secondary" timestamp={ data.timestamp } format="DD MMM, HH:mm:ss.SSS"/>
        </Flex>
      ) }
      <Flex columnGap={ 2 }>
        <Text fontWeight={ 500 }>Txn</Text>
        { data.transactions_count > 0 ? (
          <Link href={ route({
            pathname: '/block/[height_or_hash]',
            query: { height_or_hash: String(data.block_number), tab: 'txs' },
          }) }>
            { data.transactions_count }
          </Link>
        ) :
          <Text color="text.secondary">{ data.transactions_count }</Text>
        }
      </Flex>
      <Flex columnGap={ 2 }>
        <Text fontWeight={ 500 }>Gas used</Text>
        <Text color="text.secondary">{ data.gas_used.toLocaleString() }</Text>
      </Flex>
    </ListItemMobile>
  );
};

export default FlashblocksListItem;
