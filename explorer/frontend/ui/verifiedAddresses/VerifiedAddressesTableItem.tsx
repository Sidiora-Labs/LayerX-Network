import React from 'react';

import type { TokenInfoApplication, VerifiedAddress } from 'types/api/account';

import dayjs from 'lib/date/dayjs';
import { IconButton } from 'toolkit/chakra/icon-button';
import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { TableCell, TableRow } from 'toolkit/chakra/table';
import { Tooltip } from 'toolkit/chakra/tooltip';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import TokenEntity from 'ui/shared/entities/token/TokenEntity';
import IconSvg from 'ui/shared/IconSvg';

import VerifiedAddressesStatus from './VerifiedAddressesStatus';

interface Props {
  item: VerifiedAddress;
  application: TokenInfoApplication | undefined;
  onAdd: (address: string) => void;
  onEdit: (address: string) => void;
  isLoading: boolean;
}

const VerifiedAddressesTableItem = ({ item, application, onAdd, onEdit, isLoading }: Props) => {

  const handleAddClick = React.useCallback(() => {
    if (isLoading) {
      return;
    }
    onAdd(item.contractAddress);
  }, [ isLoading, item.contractAddress, onAdd ]);

  const handleEditClick = React.useCallback(() => {
    if (isLoading) {
      return;
    }
    onEdit(item.contractAddress);
  }, [ isLoading, item.contractAddress, onEdit ]);

  const tokenInfo = (() => {
    if (isLoading) {
      return <Skeleton loading height={ 6 } width="140px"/>;
    }

    if (!item.metadata.tokenName) {
      return <span>Not a token</span>;
    }

    if (!application) {
      return <Link onClick={ handleAddClick }>Add details</Link>;
    }

    const token = {
      type: 'ERC-20' as const,
      icon_url: application.iconUrl,
      address_hash: application.tokenAddress,
      name: item.metadata.tokenName,
      symbol: '',
      reputation: null,
    };

    return (
      <TokenEntity
        token={ token }
        noLink={ application.status === 'IN_PROCESS' }
        noCopy
        noSymbol
      />
    );
  })();

  return (
    <TableRow>
      <TableCell>
        <AddressEntity
          address={{ hash: item.contractAddress, is_contract: true }}
          isLoading={ isLoading }
          fontWeight="600"
        />
      </TableCell>
      <TableCell fontSize="sm" verticalAlign="middle" pr={ 1 }>
        { tokenInfo }
      </TableCell>
      <TableCell pl="0">
        { item.metadata.tokenName && application && !isLoading ? (
          <Tooltip content="Edit" disabled={ isLoading } disableOnMobile>
            <IconButton
              aria-label="edit"
              variant="link"
              size="2xs"
              borderRadius="none"
              onClick={ handleEditClick }
            >
              <IconSvg name="edit"/>
            </IconButton>
          </Tooltip>
        ) : null }
      </TableCell>
      <TableCell fontSize="sm">
        <Skeleton loading={ isLoading } display="inline-block">
          <VerifiedAddressesStatus status={ item.metadata.tokenName ? application?.status : undefined }/>
        </Skeleton>
      </TableCell>
      <TableCell fontSize="sm" color="text.secondary">
        <Skeleton loading={ isLoading } display="inline-block">
          { item.metadata.tokenName && application ? dayjs(application.updatedAt).format('MMM DD, YYYY') : null }
        </Skeleton>
      </TableCell>
    </TableRow>
  );
};

export default React.memo(VerifiedAddressesTableItem);
