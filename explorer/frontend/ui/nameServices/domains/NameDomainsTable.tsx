import React from 'react';

import type * as bens from '@blockscout/bens-types';

import { AddressHighlightProvider } from 'lib/contexts/addressHighlight';
import { TableBody, TableColumnHeader, TableColumnHeaderSortable, TableHeaderSticky, TableRoot, TableRow } from 'toolkit/chakra/table';
import { ACTION_BAR_HEIGHT_DESKTOP } from 'ui/shared/ActionBar';
import TimeFormatToggle from 'ui/shared/time/TimeFormatToggle';

import NameDomainsTableItem from './NameDomainsTableItem';
import type { SortField, Sort } from './utils';

interface Props {
  data: bens.LookupDomainNameResponse | undefined;
  isLoading?: boolean;
  sort: Sort;
  onSortToggle: (field: SortField) => void;
}

const NameDomainsTable = ({ data, isLoading, sort, onSortToggle }: Props) => {
  return (
    <AddressHighlightProvider>
      <TableRoot>
        <TableHeaderSticky top={ ACTION_BAR_HEIGHT_DESKTOP }>
          <TableRow>
            <TableColumnHeader width="25%">Domain</TableColumnHeader>
            <TableColumnHeader width="25%">Address</TableColumnHeader>
            <TableColumnHeaderSortable
              width="25%"
              pl={ 9 }
              sortField="registration_date"
              sortValue={ sort }
              onSortToggle={ onSortToggle }
              contentAfter={ <TimeFormatToggle/> }
            >
              Registered
            </TableColumnHeaderSortable>
            <TableColumnHeader width="25%">
              Expires
              <TimeFormatToggle/>
            </TableColumnHeader>
          </TableRow>
        </TableHeaderSticky>
        <TableBody>
          { data?.items.map((item, index) => <NameDomainsTableItem key={ index } { ...item } isLoading={ isLoading }/>) }
        </TableBody>
      </TableRoot>
    </AddressHighlightProvider>
  );
};

export default React.memo(NameDomainsTable);
