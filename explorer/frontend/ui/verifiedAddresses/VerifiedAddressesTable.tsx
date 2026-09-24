import React from 'react';

import type { TokenInfoApplication, VerifiedAddress } from 'types/api/account';

import { TableBody, TableColumnHeader, TableHeader, TableRoot, TableRow } from 'toolkit/chakra/table';

import VerifiedAddressesTableItem from './VerifiedAddressesTableItem';

interface Props {
  data: Array<VerifiedAddress>;
  applications: Array<TokenInfoApplication> | undefined;
  onItemAdd: (address: string) => void;
  onItemEdit: (address: string) => void;
  isLoading: boolean;
}

const VerifiedAddressesTable = ({ data, applications, onItemEdit, onItemAdd, isLoading }: Props) => {
  return (
    <TableRoot>
      <TableHeader>
        <TableRow>
          <TableColumnHeader>Address</TableColumnHeader>
          <TableColumnHeader w="168px" pr={ 1 }>Token info</TableColumnHeader>
          <TableColumnHeader w="36px" pl="0"></TableColumnHeader>
          <TableColumnHeader w="160px">Request status</TableColumnHeader>
          <TableColumnHeader w="150px">Date</TableColumnHeader>
        </TableRow>
      </TableHeader>
      <TableBody>
        { data.map((item, index) => (
          <VerifiedAddressesTableItem
            key={ item.contractAddress + (isLoading ? index : '') }
            item={ item }
            application={ applications?.find(({ tokenAddress }) => tokenAddress.toLowerCase() === item.contractAddress.toLowerCase()) }
            onAdd={ onItemAdd }
            onEdit={ onItemEdit }
            isLoading={ isLoading }
          />
        )) }
      </TableBody>
    </TableRoot>
  );
};

export default React.memo(VerifiedAddressesTable);
