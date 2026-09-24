import React from 'react';

import type {
  ValidatorStability,
  ValidatorsStabilitySortingField,
  ValidatorsStabilitySortingValue,
} from 'types/api/validators';

import { TableBody, TableColumnHeader, TableColumnHeaderSortable, TableHeaderSticky, TableRoot, TableRow } from 'toolkit/chakra/table';
import { ACTION_BAR_HEIGHT_DESKTOP } from 'ui/shared/ActionBar';
import getNextSortValue from 'ui/shared/sort/getNextSortValue';

import { VALIDATORS_STABILITY_SORT_SEQUENCE } from './utils';
import ValidatorsTableItem from './ValidatorsTableItem';

interface Props {
  data: Array<ValidatorStability>;
  sort: ValidatorsStabilitySortingValue;
  setSorting: ({ value }: { value: Array<ValidatorsStabilitySortingValue> }) => void;
  isLoading?: boolean;
}

const ValidatorsTable = ({ data, sort, setSorting, isLoading }: Props) => {

  const onSortToggle = React.useCallback((field: ValidatorsStabilitySortingField) => {
    const value = getNextSortValue<ValidatorsStabilitySortingField, ValidatorsStabilitySortingValue>(VALIDATORS_STABILITY_SORT_SEQUENCE, field)(sort);
    setSorting({ value: [ value ] });
  }, [ sort, setSorting ]);

  return (
    <TableRoot>
      <TableHeaderSticky top={ ACTION_BAR_HEIGHT_DESKTOP }>
        <TableRow>
          <TableColumnHeader width="50%">Validator’s address</TableColumnHeader>
          <TableColumnHeaderSortable
            width="25%"
            sortField="state"
            sortValue={ sort }
            onSortToggle={ onSortToggle }
          >
            Status
          </TableColumnHeaderSortable>
          <TableColumnHeaderSortable
            width="25%"
            sortField="blocks_validated"
            sortValue={ sort }
            onSortToggle={ onSortToggle }
            isNumeric
          >
            Blocks
          </TableColumnHeaderSortable>
        </TableRow>
      </TableHeaderSticky>
      <TableBody>
        { data.map((item, index) => (
          <ValidatorsTableItem
            key={ item.address.hash + (isLoading ? index : '') }
            data={ item }
            isLoading={ isLoading }/>
        )) }
      </TableBody>
    </TableRoot>
  );
};

export default React.memo(ValidatorsTable);
