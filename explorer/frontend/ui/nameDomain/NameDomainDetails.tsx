import { Flex } from '@chakra-ui/react';
import type { UseQueryResult } from '@tanstack/react-query';
import React from 'react';

import * as bens from '@blockscout/bens-types';

import { route } from 'nextjs-routes';

import config from 'configs/app';
import type { ResourceError } from 'lib/api/resources';
import { Link } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { Tooltip } from 'toolkit/chakra/tooltip';
import { stripTrailingSlash } from 'toolkit/utils/url';
import * as DetailedInfo from 'ui/shared/DetailedInfo/DetailedInfo';
import DetailedInfoTimestamp from 'ui/shared/DetailedInfo/DetailedInfoTimestamp';
import AddressEntity from 'ui/shared/entities/address/AddressEntity';
import NftEntity from 'ui/shared/entities/nft/NftEntity';
import IconSvg from 'ui/shared/IconSvg';
import TextSeparator from 'ui/shared/TextSeparator';

import NameDomainDetailsAlert from './details/NameDomainDetailsAlert';
import NameDomainExpiryStatus from './NameDomainExpiryStatus';

interface Props {
  query: UseQueryResult<bens.DetailedDomain, ResourceError<unknown>>;
}

const NameDomainDetails = ({ query }: Props) => {
  const isLoading = query.isPlaceholderData;

  const otherAddresses = Object.entries(query.data?.other_addresses ?? {});

  return (
    <>
      <NameDomainDetailsAlert data={ query.data }/>
      <DetailedInfo.Container>
        { query.data?.registration_date && (
          <>
            <DetailedInfo.ItemLabel
              hint="The date the name was registered"
              isLoading={ isLoading }
            >
              Registration date
            </DetailedInfo.ItemLabel>
            <DetailedInfo.ItemValue>
              <DetailedInfoTimestamp timestamp={ query.data.registration_date } isLoading={ isLoading }/>
            </DetailedInfo.ItemValue>
          </>
        ) }

        { query.data?.expiry_date && (
          <>
            <DetailedInfo.ItemLabel
            // eslint-disable-next-line max-len
              hint="The date the name expires, upon which there is a grace period for the owner to renew. After grace period ends, the name is released to the market"
              isLoading={ isLoading }
            >
              Expiration date
            </DetailedInfo.ItemLabel>
            <DetailedInfo.ItemValue>
              <DetailedInfoTimestamp timestamp={ query.data?.expiry_date } isLoading={ isLoading } noRelativeTime/>
              <TextSeparator/>
              <Skeleton loading={ isLoading } color="text.secondary" display="inline">
                <NameDomainExpiryStatus date={ query.data?.expiry_date }/>
              </Skeleton>
            </DetailedInfo.ItemValue>
          </>
        ) }

        { query.data?.resolver_address && (
          <>
            <DetailedInfo.ItemLabel
              hint="The resolver contract provides information about a domain name"
              isLoading={ isLoading }
            >
              Resolver
            </DetailedInfo.ItemLabel>
            <DetailedInfo.ItemValue
            >
              <AddressEntity
                address={ query.data.resolver_address }
                isLoading={ isLoading }
              />
            </DetailedInfo.ItemValue>
          </>
        ) }

        { query.data?.registrant && (
          <>
            <DetailedInfo.ItemLabel
              hint="The account that owns the domain name and has the rights to edit its ownership and records"
              isLoading={ isLoading }
            >
              Registrant
            </DetailedInfo.ItemLabel>
            <DetailedInfo.ItemValue
              columnGap={ 2 }
              flexWrap="nowrap"
            >
              <AddressEntity
                address={ query.data.registrant }
                isLoading={ isLoading }
              />
              <Tooltip content="Lookup for related domain names">
                <Link
                  flexShrink={ 0 }
                  display="inline-flex"
                  href={ route({
                    pathname: '/name-services',
                    query: { tab: 'domains', owned_by: 'true', resolved_to: 'true', address: query.data.registrant.hash },
                  }) }
                >
                  <IconSvg name="search" boxSize={ 5 } isLoading={ isLoading }/>
                </Link>
              </Tooltip>
            </DetailedInfo.ItemValue>
          </>
        ) }

        { query.data?.owner && (
          <>
            <DetailedInfo.ItemLabel
              hint="The account that owns the rights to edit the records of this domain name"
              isLoading={ isLoading }
            >
              Owner
            </DetailedInfo.ItemLabel>
            <DetailedInfo.ItemValue
              columnGap={ 2 }
              flexWrap="nowrap"
            >
              <AddressEntity
                address={ query.data.owner }
                isLoading={ isLoading }
              />
              <Tooltip content="Lookup for related domain names">
                <Link
                  flexShrink={ 0 }
                  display="inline-flex"
                  href={ route({
                    pathname: '/name-services',
                    query: { tab: 'domains', owned_by: 'true', resolved_to: 'true', address: query.data.owner.hash },
                  }) }
                >
                  <IconSvg name="search" boxSize={ 5 } isLoading={ isLoading }/>
                </Link>
              </Tooltip>
            </DetailedInfo.ItemValue>
          </>
        ) }

        { query.data?.wrapped_owner && (
          <>
            <DetailedInfo.ItemLabel
              hint="Owner of this NFT domain in NameWrapper contract"
              isLoading={ isLoading }
            >
              Manager
            </DetailedInfo.ItemLabel>
            <DetailedInfo.ItemValue
              columnGap={ 2 }
              flexWrap="nowrap"
            >
              <AddressEntity
                address={ query.data.wrapped_owner }
                isLoading={ isLoading }
              />
              <Tooltip content="Lookup for related domain names">
                <Link
                  flexShrink={ 0 }
                  display="inline-flex"
                  href={ route({
                    pathname: '/name-services',
                    query: { tab: 'domains', owned_by: 'true', resolved_to: 'true', address: query.data.wrapped_owner.hash },
                  }) }
                >
                  <IconSvg name="search" boxSize={ 5 } isLoading={ isLoading }/>
                </Link>
              </Tooltip>
            </DetailedInfo.ItemValue>
          </>
        ) }

        { query.data?.tokens.map((token) => {
          const isProtocolBaseChain = stripTrailingSlash(query.data.protocol?.deployment_blockscout_base_url ?? '') === config.app.baseUrl;
          const entityProps = {
            link: { external: !isProtocolBaseChain ? true : false },
            href: !isProtocolBaseChain ? (
              stripTrailingSlash(query.data.protocol?.deployment_blockscout_base_url ?? '') +
            route({ pathname: '/token/[hash]/instance/[id]', query: { hash: token.contract_hash, id: token.id } })
            ) : undefined,
          };

          return (
            <React.Fragment key={ token.type }>
              <DetailedInfo.ItemLabel
                hint={ `The ${ token.type === bens.TokenType.WRAPPED_DOMAIN_TOKEN ? 'wrapped ' : '' }token ID of this domain name NFT` }
                isLoading={ isLoading }
              >
                { token.type === bens.TokenType.WRAPPED_DOMAIN_TOKEN ? 'Wrapped token ID' : 'Token ID' }
              </DetailedInfo.ItemLabel>
              <DetailedInfo.ItemValue
                wordBreak="break-all"
                whiteSpace="pre-wrap"
              >
                <NftEntity { ...entityProps } hash={ token.contract_hash } id={ token.id } isLoading={ isLoading } noIcon/>
              </DetailedInfo.ItemValue>
            </React.Fragment>
          );
        }) }

        { otherAddresses.length > 0 && (
          <>
            <DetailedInfo.ItemLabel
              hint="Other cryptocurrency addresses added to this domain name"
              isLoading={ isLoading }
            >
              Other addresses
            </DetailedInfo.ItemLabel>
            <DetailedInfo.ItemValue
              flexDir="column"
              alignItems="flex-start"
              multiRow
            >
              { otherAddresses.map(([ type, address ]) => (
                <Flex key={ type } columnGap={ 2 } minW="0" w="100%" overflow="hidden">
                  <Skeleton loading={ isLoading }>{ type }</Skeleton>
                  <AddressEntity
                    address={{ hash: address }}
                    isLoading={ isLoading }
                    noLink
                    noIcon
                  />
                </Flex>
              )) }
            </DetailedInfo.ItemValue>
          </>
        ) }
      </DetailedInfo.Container>
    </>
  );
};

export default React.memo(NameDomainDetails);
