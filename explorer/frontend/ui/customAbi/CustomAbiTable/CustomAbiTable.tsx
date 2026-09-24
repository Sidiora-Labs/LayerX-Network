import React from 'react';

import type { CustomAbis, CustomAbi } from 'types/api/account';

import { TableBody, TableColumnHeader, TableHeader, TableRoot, TableRow } from 'toolkit/chakra/table';

import CustomAbiTableItem from './CustomAbiTableItem';

interface Props {
  data?: CustomAbis;
  isLoading?: boolean;
  onEditClick: (item: CustomAbi) => void;
  onDeleteClick: (item: CustomAbi) => void;
}

const CustomAbiTable = ({ data, isLoading, onDeleteClick, onEditClick }: Props) => {
  return (
    <TableRoot minWidth="600px">
      <TableHeader>
        <TableRow>
          <TableColumnHeader>ABI for Smart contract address (0x...)</TableColumnHeader>
          <TableColumnHeader width="108px"></TableColumnHeader>
        </TableRow>
      </TableHeader>
      <TableBody>
        { data?.map((item, index) => (
          <CustomAbiTableItem
            key={ item.id + (isLoading ? String(index) : '') }
            item={ item }
            isLoading={ isLoading }
            onDeleteClick={ onDeleteClick }
            onEditClick={ onEditClick }
          />
        )) }
      </TableBody>
    </TableRoot>
  );
};

export default React.memo(CustomAbiTable);
