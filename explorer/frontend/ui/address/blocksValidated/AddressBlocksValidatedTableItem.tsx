import { Flex } from '@chakra-ui/react';
import BigNumber from 'bignumber.js';
import React from 'react';

import type { Block } from 'types/api/block';

import config from 'configs/app';
import getBlockTotalReward from 'lib/block/getBlockTotalReward';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { TableCell, TableRow } from 'toolkit/chakra/table';
import BlockGasUsed from 'ui/shared/block/BlockGasUsed';
import BlockEntity from 'ui/shared/entities/block/BlockEntity';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';
import SimpleValue from 'ui/shared/value/SimpleValue';

type Props = Block & {
  page: number;
  isLoading: boolean;
};

const AddressBlocksValidatedTableItem = (props: Props) => {
  const totalReward = getBlockTotalReward(props);

  return (
    <TableRow>
      <TableCell>
        <BlockEntity
          isLoading={ props.isLoading }
          number={ props.height }
          noIcon
          textStyle="sm"
          fontWeight={ 700 }
        />
      </TableCell>
      <TableCell>
        <TimeWithTooltip
          timestamp={ props.timestamp }
          enableIncrement={ props.page === 1 }
          isLoading={ props.isLoading }
          color="text.secondary"
          display="inline-block"
        />
      </TableCell>
      <TableCell>
        <Skeleton loading={ props.isLoading } display="inline-block" fontWeight="500">
          <span>{ props.transactions_count }</span>
        </Skeleton>
      </TableCell>
      <TableCell>
        <Flex alignItems="center" columnGap={ 2 }>
          <Skeleton loading={ props.isLoading } flexBasis="80px">
            { BigNumber(props.gas_used || 0).toFormat() }
          </Skeleton>
          <BlockGasUsed
            gasUsed={ props.gas_used || undefined }
            gasLimit={ props.gas_limit }
            isLoading={ props.isLoading }
          />
        </Flex>
      </TableCell>
      { !config.UI.views.block.hiddenFields?.total_reward && !config.features.rollup.isEnabled && (
        <TableCell isNumeric>
          <SimpleValue
            value={ totalReward }
            accuracy={ 0 }
            loading={ props.isLoading }
          />
        </TableCell>
      ) }
    </TableRow>
  );
};

export default React.memo(AddressBlocksValidatedTableItem);
