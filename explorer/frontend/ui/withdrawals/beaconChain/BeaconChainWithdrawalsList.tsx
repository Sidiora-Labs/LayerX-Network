import { Box } from '@chakra-ui/react';
import React from 'react';

import type { AddressWithdrawalsItem } from 'types/api/address';
import type { BlockWithdrawalsItem } from 'types/api/block';
import type { WithdrawalsItem } from 'types/api/withdrawals';

import useLazyRenderedList from 'lib/hooks/useLazyRenderedList';

import BeaconChainWithdrawalsListItem from './BeaconChainWithdrawalsListItem';

type Props = {
  isLoading?: boolean;
} & ({
  items: Array<WithdrawalsItem>;
  view: 'list';
} | {
  items: Array<AddressWithdrawalsItem>;
  view: 'address';
} | {
  items: Array<BlockWithdrawalsItem>;
  view: 'block';
});

const WithdrawalsList = ({ items, view, isLoading }: Props) => {
  const { cutRef, renderedItemsNum } = useLazyRenderedList(items, !isLoading);

  return (
    <Box>
      { items.slice(0, renderedItemsNum).map((item, index) => {

        const key = item.index + (isLoading ? String(index) : '');

        switch (view) {
          case 'address': {
            return (
              <BeaconChainWithdrawalsListItem
                key={ key }
                item={ item as AddressWithdrawalsItem }
                view={ view }
                isLoading={ isLoading }
              />
            );
          }
          case 'block': {
            return (
              <BeaconChainWithdrawalsListItem
                key={ key }
                item={ item as BlockWithdrawalsItem }
                view={ view }
                isLoading={ isLoading }
              />
            );
          }
          case 'list': {
            return (
              <BeaconChainWithdrawalsListItem
                key={ key }
                item={ item as WithdrawalsItem }
                view={ view }
                isLoading={ isLoading }
              />
            );
          }
        }
      }) }
      <div ref={ cutRef }/>
    </Box>
  );
};

export default React.memo(WithdrawalsList);
