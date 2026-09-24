import { Box, Flex, Grid, chakra } from '@chakra-ui/react';
import type { UseQueryResult } from '@tanstack/react-query';
import { clamp } from 'es-toolkit';
import React from 'react';

import type * as bens from '@blockscout/bens-types';

import { route } from 'nextjs-routes';

import type { ResourceError } from 'lib/api/resources';
import dayjs from 'lib/date/dayjs';
import { Button } from 'toolkit/chakra/button';
import { Link } from 'toolkit/chakra/link';
import { PopoverBody, PopoverContent, PopoverRoot, PopoverTrigger } from 'toolkit/chakra/popover';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { Tooltip } from 'toolkit/chakra/tooltip';
import { useDisclosure } from 'toolkit/hooks/useDisclosure';
import EnsEntity from 'ui/shared/entities/ens/EnsEntity';
import IconSvg from 'ui/shared/IconSvg';

interface Props {
  query: UseQueryResult<bens.LookupAddressResponse, ResourceError<unknown>>;
  addressHash: string;
  mainDomainName: string | null | undefined;
}

const DomainsGrid = ({ data }: { data: Array<bens.Domain> }) => {
  return (
    <Grid
      templateColumns={{ base: `repeat(${ clamp(data.length, 1, 2) }, 1fr)`, lg: `repeat(${ clamp(data.length, 1, 3) }, 1fr)` }}
      columnGap={ 8 }
      rowGap={ 4 }
      mt={ 2 }
    >
      { data.slice(0, 9).map((domain) => <EnsEntity key={ domain.id } domain={ domain.name } protocol={ domain.protocol } noCopy/>) }
    </Grid>
  );
};

const AddressEnsDomains = ({ query, addressHash, mainDomainName }: Props) => {
  const { data, isPending, isError } = query;
  const popover = useDisclosure();

  if (isError) {
    return null;
  }

  if (isPending) {
    return <Skeleton loading h={ 8 } w={{ base: '50px', xl: '120px' }} borderRadius="base"/>;
  }

  if (!data || data.items.length === 0) {
    return null;
  }

  const mainDomain = data.items.find((domain) => mainDomainName && domain.name === mainDomainName);
  const ownedDomains = data.items.filter((domain) => {
    if (mainDomainName && domain.name === mainDomainName) {
      return false;
    }

    // exclude resolved address
    if (domain.resolved_address && domain.resolved_address.hash.toLowerCase() === addressHash.toLowerCase()) {
      return false;
    }

    if (domain.owner && domain.owner.hash.toLowerCase() === addressHash.toLowerCase()) {
      return true;
    }

    // include wrapped owner
    if (domain.wrapped_owner?.hash.toLowerCase() === addressHash.toLowerCase()) {
      return !domain.resolved_address || domain.resolved_address.hash.toLowerCase() !== addressHash.toLowerCase();
    }

    return false;
  });
  const resolvedDomains = data.items.filter((domain) =>
    domain.resolved_address &&
    domain.resolved_address.hash.toLowerCase() === addressHash.toLowerCase() &&
    domain.name !== mainDomainName,
  );

  const totalRecords = data.items.length > 40 ? '40+' : data.items.length;

  return (
    <PopoverRoot open={ popover.open } onOpenChange={ popover.onOpenChange }>
      <Tooltip content="List of names resolved or owned by this address" disabled={ popover.open } disableOnMobile closeOnClick>
        <div>
          <PopoverTrigger>
            <Button
              size="sm"
              variant="dropdown"
              aria-label="Address domains"
              fontWeight={ 500 }
              flexShrink={ 0 }
              columnGap={ 1 }
            >
              <IconSvg name="ENS" boxSize={ 5 }/>
              <chakra.span hideBelow="xl">{ totalRecords } Domain{ data.items.length > 1 ? 's' : '' }</chakra.span>
              <chakra.span hideFrom="xl">{ totalRecords }</chakra.span>
            </Button>
          </PopoverTrigger>
        </div>
      </Tooltip>
      <PopoverContent w={{ lg: '500px' }}>
        <PopoverBody textStyle="sm" display="flex" flexDir="column" rowGap={ 5 } alignItems="flex-start">
          { mainDomain && (
            <Box w="100%">
              <chakra.span color="text.secondary" textStyle="xs">Primary*</chakra.span>
              <Flex alignItems="center" textStyle="md" mt={ 2 }>
                <EnsEntity domain={ mainDomain.name } protocol={ mainDomain.protocol } fontWeight={ 600 } noCopy/>
                { mainDomain.expiry_date &&
                    <chakra.span color="text.secondary" whiteSpace="pre"> (expires { dayjs(mainDomain.expiry_date).fromNow() })</chakra.span> }
              </Flex>
            </Box>
          ) }
          { ownedDomains.length > 0 && (
            <div>
              <chakra.span color="text.secondary" textStyle="xs">Owned by this address</chakra.span>
              <DomainsGrid data={ ownedDomains }/>
            </div>
          ) }
          { resolvedDomains.length > 0 && (
            <div>
              <chakra.span color="text.secondary" textStyle="xs">Resolved to this address</chakra.span>
              <DomainsGrid data={ resolvedDomains }/>
            </div>
          ) }
          { (ownedDomains.length > 9 || resolvedDomains.length > 9) && (
            <Link
              href={ route({ pathname: '/name-services', query: { tab: 'domains', owned_by: 'true', resolved_to: 'true', address: addressHash } }) }
            >
              <span> More results</span>
              <chakra.span color="text.secondary"> ({ totalRecords })</chakra.span>
            </Link>
          ) }
          { mainDomain && (
            <chakra.span textStyle="xs" mt={ -1 }>
              *A domain name is not necessarily held by a person popularly associated with the name
            </chakra.span>
          ) }
        </PopoverBody>
      </PopoverContent>
    </PopoverRoot>
  );
};

export default React.memo(AddressEnsDomains);
