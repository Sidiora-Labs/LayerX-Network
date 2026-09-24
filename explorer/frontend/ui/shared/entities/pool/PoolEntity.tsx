import { Flex, chakra } from '@chakra-ui/react';
import React from 'react';

import type { Pool } from 'types/api/pools';

import { route } from 'nextjs-routes';

import { getPoolTitle } from 'lib/pools/getPoolTitle';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { TruncatedTextTooltip } from 'toolkit/components/truncation/TruncatedTextTooltip';
import * as EntityBase from 'ui/shared/entities/base/components';

import { distributeEntityProps } from '../base/utils';
import * as TokenEntity from '../token/TokenEntity';

type LinkProps = EntityBase.LinkBaseProps & Pick<EntityProps, 'pool'>;

const Link = chakra((props: LinkProps) => {
  const defaultHref = route({ pathname: '/pools/[hash]', query: { hash: props.pool.pool_id } });

  return (
    <EntityBase.Link
      { ...props }
      href={ props.href ?? defaultHref }
    >
      { props.children }
    </EntityBase.Link>
  );
});

type IconProps = Pick<EntityProps, 'pool' | 'className'> & EntityBase.IconBaseProps;

const Icon = (props: IconProps) => {
  const borderColor = { _light: 'whiteAlpha.800', _dark: 'blackAlpha.800' };
  return (
    <Flex>
      <Flex
        bgColor="bg.primary"
        borderRadius="full"
        border="1px solid"
        borderColor={ borderColor }
      >
        <TokenEntity.Icon
          marginRight={ 0 }
          variant={ props.variant }
          token={{
            icon_url: props.pool.base_token_icon_url,
            symbol: props.pool.base_token_symbol,
            address_hash: props.pool.base_token_address,
            name: '',
            type: 'ERC-20',
            reputation: null,
          }}
          isLoading={ props.isLoading }
        />
      </Flex>
      <Flex
        transform="translateX(-8px)"
        bgColor="bg.primary"
        borderRadius="full"
        border="1px solid"
        borderColor={ borderColor }
      >
        <TokenEntity.Icon
          marginRight={ 0 }
          variant={ props.variant }
          token={{
            icon_url: props.pool.quote_token_icon_url,
            symbol: props.pool.quote_token_symbol,
            address_hash: props.pool.quote_token_address,
            name: '',
            type: 'ERC-20',
            reputation: null,
          }}
          isLoading={ props.isLoading }
        />
      </Flex>
    </Flex>
  );
};

type ContentProps = Omit<EntityBase.ContentBaseProps, 'text'> & Pick<EntityProps, 'pool'>;

const Content = chakra((props: ContentProps) => {
  const nameString = getPoolTitle(props.pool);

  return (
    <TruncatedTextTooltip label={ nameString }>
      <Skeleton
        loading={ props.isLoading }
        display="inline-block"
        whiteSpace="nowrap"
        overflow="hidden"
        textOverflow="ellipsis"
        height="fit-content"
      >
        { nameString }
      </Skeleton>
    </TruncatedTextTooltip>
  );
});

const Container = EntityBase.Container;

export interface EntityProps extends EntityBase.EntityBaseProps {
  pool: Pool;
}

const PoolEntity = (props: EntityProps) => {
  const partsProps = distributeEntityProps(props);
  const content = <Content { ...partsProps.content }/>;

  return (
    <Container w="100%" { ...partsProps.container }>
      <Icon { ...partsProps.icon }/>
      { props.noLink ? content : <Link { ...partsProps.link }>{ content }</Link> }
    </Container>
  );
};

export default React.memo(chakra(PoolEntity));

export {
  Container,
  Link,
  Icon,
  Content,
};
