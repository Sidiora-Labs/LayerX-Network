import { Flex } from '@chakra-ui/react';
import React from 'react';

import type { TxAuthorization } from 'types/api/transaction';

import config from 'configs/app';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { TableRow, TableCell } from 'toolkit/chakra/table';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import TxAuthorizationStatus from 'ui/shared/statusTag/TxAuthorizationStatus';

interface Props extends TxAuthorization {
  isLoading?: boolean;
}

const TxAuthorizationsItem = ({ address_hash: addressHash, authority, chain_id: chainId, nonce, isLoading, status }: Props) => {
  return (
    <TableRow alignItems="top">
      <TableCell>
        <Flex>
          <AddressEntity address={{ hash: addressHash }} isLoading={ isLoading } noIcon/>
        </Flex>
      </TableCell>
      <TableCell verticalAlign="middle">
        <Flex>
          <AddressEntity address={{ hash: authority }} isLoading={ isLoading } noIcon/>
        </Flex>
      </TableCell>
      <TableCell verticalAlign="middle">
        <Skeleton loading={ isLoading } display="inline-block">
          { chainId === Number(config.chain.id) ? 'this' : 'any' }
        </Skeleton>
      </TableCell>
      <TableCell verticalAlign="middle">
        <Skeleton loading={ isLoading } display="inline-block">
          { nonce }
        </Skeleton>
      </TableCell>
      <TableCell verticalAlign="middle">
        <TxAuthorizationStatus status={ status } loading={ isLoading }/>
      </TableCell>
    </TableRow>
  );
};

export default React.memo(TxAuthorizationsItem);
