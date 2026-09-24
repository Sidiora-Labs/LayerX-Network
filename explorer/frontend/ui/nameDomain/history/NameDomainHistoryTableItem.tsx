import React from 'react';

import type * as bens from '@blockscout/bens-types';

import { route } from 'nextjs-routes';

import config from 'configs/app';
import { Badge } from 'toolkit/chakra/badge';
import { TableCell, TableRow } from 'toolkit/chakra/table';
import { stripTrailingSlash } from 'toolkit/utils/url';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import TxEntity from 'ui/shared/entities/tx/TxEntity';
import TimeWithTooltip from 'ui/shared/time/TimeWithTooltip';

interface Props {
  event: bens.DomainEvent;
  domain: bens.DetailedDomain | undefined;
  isLoading?: boolean;
}

const NameDomainHistoryTableItem = ({ isLoading, event, domain }: Props) => {
  const isProtocolBaseChain = stripTrailingSlash(domain?.protocol?.deployment_blockscout_base_url ?? '') === config.app.baseUrl;
  const txEntityProps = {
    link: { external: !isProtocolBaseChain ? true : false },
    href: !isProtocolBaseChain ? (
      stripTrailingSlash(domain?.protocol?.deployment_blockscout_base_url ?? '') +
      route({ pathname: '/tx/[hash]', query: { hash: event.transaction_hash } })
    ) : undefined,
  };

  return (
    <TableRow>
      <TableCell verticalAlign="middle">
        <TxEntity
          { ...txEntityProps }
          hash={ event.transaction_hash }
          isLoading={ isLoading }
          fontWeight={ 700 }
          noIcon
          truncation="constant_long"
          noCopy
        />
      </TableCell>
      <TableCell pl={ 9 } verticalAlign="middle">
        <TimeWithTooltip
          timestamp={ event.timestamp }
          isLoading={ isLoading }
          color="text.secondary"
          display="inline-block"
        />
      </TableCell>
      <TableCell verticalAlign="middle">
        { event.from_address && <AddressEntity address={ event.from_address } isLoading={ isLoading } truncation="constant"/> }
      </TableCell>
      <TableCell verticalAlign="middle">
        { event.action && <Badge colorPalette="gray" loading={ isLoading }>{ event.action }</Badge> }
      </TableCell>
    </TableRow>
  );
};

export default React.memo(NameDomainHistoryTableItem);
