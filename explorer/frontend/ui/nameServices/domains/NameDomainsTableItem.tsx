import React from 'react';

import type * as bens from '@blockscout/bens-types';

import dayjs from 'lib/date/dayjs';
import { TableCell, TableRow } from 'toolkit/chakra/table';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import EnsEntity from 'ui/shared/entities/ens/EnsEntity';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';

type Props = bens.Domain & {
  isLoading?: boolean;
};

const NameDomainsTableItem = ({
  isLoading,
  name,
  resolved_address: resolvedAddress,
  registration_date: registrationDate,
  expiry_date: expiryDate,
  protocol,
}: Props) => {

  return (
    <TableRow>
      <TableCell verticalAlign="middle">
        <EnsEntity domain={ name } protocol={ protocol } isLoading={ isLoading } fontWeight={ 600 }/>
      </TableCell>
      <TableCell verticalAlign="middle">
        { resolvedAddress && <AddressEntity address={ resolvedAddress } isLoading={ isLoading } fontWeight={ 500 }/> }
      </TableCell>
      <TableCell verticalAlign="middle" pl={ 9 }>
        <TimeWithTooltip timestamp={ registrationDate } isLoading={ isLoading }/>
      </TableCell>
      <TableCell verticalAlign="middle">
        <TimeWithTooltip
          timestamp={ expiryDate }
          isLoading={ isLoading }
          color={ expiryDate && dayjs(expiryDate).diff(dayjs(), 'day') < 30 ? 'red.600' : 'inherit' }
        />
      </TableCell>
    </TableRow>
  );
};

export default React.memo(NameDomainsTableItem);
