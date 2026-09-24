import { Text, Flex } from '@chakra-ui/react';
import BigNumber from 'bignumber.js';
import React from 'react';

import type { Block } from 'types/api/block';

import config from 'configs/app';
import getBlockTotalReward from 'lib/block/getBlockTotalReward';
import { currencyUnits } from 'lib/units';
import { Skeleton } from 'toolkit/chakra/skeleton';
import BlockGasUsed from 'ui/shared/block/BlockGasUsed';
import BlockEntity from 'ui/shared/entities/block/BlockEntity';
import ListItemMobile from 'ui/shared/ListItemMobile/ListItemMobile';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';
import SimpleValue from 'ui/shared/value/SimpleValue';

type Props = Block & {
  page: number;
  isLoading: boolean;
};

const AddressBlocksValidatedListItem = (props: Props) => {
  const totalReward = getBlockTotalReward(props);

  return (
    <ListItemMobile rowGap={ 2 }>
      <Flex justifyContent="space-between" w="100%">
        <BlockEntity
          isLoading={ props.isLoading }
          number={ props.height }
          noIcon
          fontWeight={ 700 }
        />
        <TimeWithTooltip
          timestamp={ props.timestamp }
          enableIncrement={ props.page === 1 }
          isLoading={ props.isLoading }
          color="text.secondary"
          display="inline-block"
        />
      </Flex>
      <Flex columnGap={ 2 } w="100%">
        <Skeleton loading={ props.isLoading } fontWeight={ 500 } flexShrink={ 0 }>Txn</Skeleton>
        <Skeleton loading={ props.isLoading } display="inline-block" color="Skeleton_secondary">
          <span>{ props.transactions_count }</span>
        </Skeleton>
      </Flex>
      <Flex columnGap={ 2 } w="100%">
        <Skeleton loading={ props.isLoading } fontWeight={ 500 } flexShrink={ 0 }>Gas used</Skeleton>
        <Skeleton loading={ props.isLoading }>
          <Text color="text.secondary">{ BigNumber(props.gas_used || 0).toFormat() }</Text>
        </Skeleton>
        <BlockGasUsed
          gasUsed={ props.gas_used || undefined }
          gasLimit={ props.gas_limit }
          isLoading={ props.isLoading }
        />
      </Flex>
      { !config.UI.views.block.hiddenFields?.total_reward && !config.features.rollup.isEnabled && (
        <Flex columnGap={ 2 } w="100%">
          <Skeleton loading={ props.isLoading } fontWeight={ 500 } flexShrink={ 0 }>Reward { currencyUnits.ether }</Skeleton>
          <SimpleValue
            value={ totalReward }
            accuracy={ 0 }
            loading={ props.isLoading }
            color="text.secondary"
          />
        </Flex>
      ) }
    </ListItemMobile>
  );
};

export default React.memo(AddressBlocksValidatedListItem);
