import { Flex } from '@chakra-ui/react';
import React from 'react';

import type { PaxeerXReceiptsItem } from 'types/api/paxeerXLists';

import { route } from 'nextjs/routes';

import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import CopyToClipboard from 'ui/shared/CopyToClipboard';
import BlockEntity from 'ui/shared/entities/block/BlockEntity';
import HashStringShorten from 'ui/shared/HashStringShorten';
import ListItemMobileGrid from 'ui/shared/ListItemMobile/ListItemMobileGrid';
import StatusLadderBadge from 'ui/shared/statusLadder/StatusLadderBadge';

interface Props {
  item: PaxeerXReceiptsItem;
  isLoading?: boolean;
}

const PaxeerXReceiptsListItem = ({ item, isLoading }: Props) => {
  return (
    <ListItemMobileGrid.Container>

      <ListItemMobileGrid.Label isLoading={ isLoading }>Receipt ID</ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value>
        <Flex overflow="hidden" whiteSpace="nowrap" alignItems="center" w="100%" justifyContent="start">
          <Skeleton loading={ isLoading } fontWeight={ 600 } color="text.primary">
            <HashStringShorten hash={ item.id } type="long"/>
          </Skeleton>
          <CopyToClipboard text={ item.id } isLoading={ isLoading }/>
        </Flex>
      </ListItemMobileGrid.Value>

      <ListItemMobileGrid.Label isLoading={ isLoading }>Account</ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value>
        <Skeleton loading={ isLoading } overflow="hidden">
          <Link href={ route({ pathname: '/paxeer-x/account/[hash]', query: { hash: item.account } }) }>
            <HashStringShorten hash={ item.account } type="long"/>
          </Link>
        </Skeleton>
      </ListItemMobileGrid.Value>

      <ListItemMobileGrid.Label isLoading={ isLoading }>Status</ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value>
        <StatusLadderBadge rung={ item.status } isLoading={ isLoading }/>
      </ListItemMobileGrid.Value>

      <ListItemMobileGrid.Label isLoading={ isLoading }>Block</ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value>
        <BlockEntity
          isLoading={ isLoading }
          number={ item.block_number }
          noIcon
        />
      </ListItemMobileGrid.Value>

    </ListItemMobileGrid.Container>
  );
};

export default PaxeerXReceiptsListItem;
