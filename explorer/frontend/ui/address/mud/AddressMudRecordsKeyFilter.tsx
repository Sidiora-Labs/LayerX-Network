import React from 'react';

import TableColumnFilterWrapper from 'ui/shared/filters/TableColumnFilterWrapper';

import AddressMudRecordsKeyFilterContent from './AddressMudRecordsKeyFilterContent';

type Props = {
  value?: string;
  handleFilterChange: (val: string) => void;
  title: string;
  columnName: string;
  isLoading?: boolean;
};

const AddressMudRecordsKeyFilter = ({ value = '', handleFilterChange, columnName, title, isLoading }: Props) => {
  return (
    <TableColumnFilterWrapper
      columnName={ columnName }
      selected={ Boolean(value) }
      isLoading={ isLoading }
      w="350px"
    >
      <AddressMudRecordsKeyFilterContent
        value={ value }
        handleFilterChange={ handleFilterChange }
        title={ title }
        columnName={ columnName }
      />
    </TableColumnFilterWrapper>
  );
};

export default AddressMudRecordsKeyFilter;
