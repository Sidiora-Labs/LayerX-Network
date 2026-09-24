import React from 'react';

import { FilterInput } from 'toolkit/components/filters/FilterInput';
import TableColumnFilter from 'ui/shared/filters/TableColumnFilter';

type Props = {
  value?: string;
  handleFilterChange: (val: string) => void;
  title: string;
  columnName: string;
};

const AddressMudRecordsKeyFilterContent = ({ value = '', handleFilterChange, columnName, title }: Props) => {
  const [ filterValue, setFilterValue ] = React.useState<string>(value);

  const onFilter = React.useCallback(() => {
    handleFilterChange(filterValue);
  }, [ handleFilterChange, filterValue ]);

  return (
    <TableColumnFilter
      title={ title }
      isFilled={ Boolean(filterValue) }
      onFilter={ onFilter }
      isTouched={ filterValue !== value }
    >
      <FilterInput
        initialValue={ value }
        size="sm"
        onChange={ setFilterValue }
        placeholder={ columnName }
      />
    </TableColumnFilter>
  );
};

export default AddressMudRecordsKeyFilterContent;
