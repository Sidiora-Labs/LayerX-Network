import React from 'react';

import type * as bens from '@blockscout/bens-types';

import dayjs from 'lib/date/dayjs';
import { Skeleton } from 'toolkit/chakra/skeleton';
import NameDomainExpiryStatus from 'ui/nameDomain/NameDomainExpiryStatus';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import EnsEntity from 'ui/shared/entities/ens/EnsEntity';
import ListItemMobileGrid from 'ui/shared/ListItemMobile/ListItemMobileGrid';
import Time from 'ui/shared/time/Time';

interface Props extends bens.Domain {
  isLoading: boolean;
}

const NameDomainsListItem = ({
  name,
  isLoading,
  resolved_address: resolvedAddress,
  registration_date: registrationDate,
  expiry_date: expiryDate,
  protocol,
}: Props) => {
  return (
    <ListItemMobileGrid.Container>
      <ListItemMobileGrid.Label isLoading={ isLoading }>Domain</ListItemMobileGrid.Label>
      <ListItemMobileGrid.Value>
        <EnsEntity domain={ name } protocol={ protocol } isLoading={ isLoading } fontWeight={ 500 }/>
      </ListItemMobileGrid.Value>

      { resolvedAddress && (
        <>
          <ListItemMobileGrid.Label isLoading={ isLoading }>Address</ListItemMobileGrid.Label>
          <ListItemMobileGrid.Value>
            <AddressEntity address={ resolvedAddress } isLoading={ isLoading } fontWeight={ 500 }/>
          </ListItemMobileGrid.Value>
        </>
      ) }

      { registrationDate && (
        <>
          <ListItemMobileGrid.Label isLoading={ isLoading }>Registered on</ListItemMobileGrid.Label>
          <ListItemMobileGrid.Value>
            <Skeleton loading={ isLoading }>
              <Time timestamp={ registrationDate }/>
              <div> { dayjs(registrationDate).fromNow() }</div>
            </Skeleton>
          </ListItemMobileGrid.Value>
        </>
      ) }

      { expiryDate && (
        <>
          <ListItemMobileGrid.Label isLoading={ isLoading }>Expiration date</ListItemMobileGrid.Label>
          <ListItemMobileGrid.Value>
            <Skeleton loading={ isLoading } whiteSpace="pre-wrap">
              <Time timestamp={ expiryDate } display="block"/>
              <NameDomainExpiryStatus date={ expiryDate }/>
            </Skeleton>
          </ListItemMobileGrid.Value>
        </>
      ) }
    </ListItemMobileGrid.Container>
  );
};

export default React.memo(NameDomainsListItem);
