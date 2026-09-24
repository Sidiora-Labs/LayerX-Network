import { Flex } from '@chakra-ui/react';
import React from 'react';

import type { AddressEpochRewardsItem } from 'types/api/address';

import { route } from 'nextjs-routes';

import { Link } from 'toolkit/chakra/link';
import { TableCell, TableRow } from 'toolkit/chakra/table';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import EpochRewardTypeTag from 'ui/shared/EpochRewardTypeTag';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';
import TokenValue from 'ui/shared/value/TokenValue';

type Props = {
  item: AddressEpochRewardsItem;
  isLoading?: boolean;
};

const AddressEpochRewardsTableItem = ({ item, isLoading }: Props) => {
  return (
    <TableRow>
      <TableCell verticalAlign="middle">
        <Flex alignItems="center" gap={ 3 }>
          <Link
            href={ route({ pathname: '/epochs/[number]', query: { number: String(item.epoch_number) } }) }
            loading={ isLoading }
          >
            { item.epoch_number }
          </Link>
          <TimeWithTooltip timestamp={ item.block_timestamp } isLoading={ isLoading } color="text.secondary" fontWeight={ 400 }/>
        </Flex>
      </TableCell>
      <TableCell verticalAlign="middle">
        <EpochRewardTypeTag type={ item.type } isLoading={ isLoading }/>
      </TableCell>
      <TableCell verticalAlign="middle">
        <AddressEntity address={ item.associated_account } isLoading={ isLoading } truncation="constant"/>
      </TableCell>
      <TableCell verticalAlign="middle" isNumeric>
        <TokenValue
          amount={ item.amount }
          token={ item.token }
          loading={ isLoading }
          layout="vertical"
          rowGap="10px"
        />
      </TableCell>
    </TableRow>
  );
};

export default AddressEpochRewardsTableItem;
