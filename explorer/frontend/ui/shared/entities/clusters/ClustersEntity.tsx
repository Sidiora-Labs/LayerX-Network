import { Box, chakra, Flex, Text } from '@chakra-ui/react';
import React from 'react';

import { route } from 'nextjs-routes';

import config from 'configs/app';
import { Image } from 'toolkit/chakra/image';
import { Link as LinkToolkit } from 'toolkit/chakra/link';
import { Skeleton } from 'toolkit/chakra/skeleton';
import { Tooltip } from 'toolkit/chakra/tooltip';
import * as EntityBase from 'ui/shared/entities/base/components';
import IconSvg from 'ui/shared/IconSvg';

import { distributeEntityProps, getIconProps } from '../base/utils';

const nameServicesFeature = config.features.nameServices;
const clustersFeature = nameServicesFeature.isEnabled && nameServicesFeature.clusters.isEnabled ? nameServicesFeature.clusters : undefined;

type LinkProps = EntityBase.LinkBaseProps & Pick<EntityProps, 'clusterName'>;

const Link = chakra((props: LinkProps) => {
  const defaultHref = route({ pathname: '/name-services/clusters/[name]', query: { name: encodeURIComponent(props.clusterName) } });

  return (
    <EntityBase.Link
      { ...props }
      href={ props.href ?? defaultHref }
    >
      { props.children }
    </EntityBase.Link>
  );
});

type IconProps = EntityBase.IconBaseProps & Pick<EntityProps, 'clusterName'>;

const Icon = (props: IconProps) => {
  if (props.noIcon) {
    return null;
  }

  const styles = getIconProps(props, Boolean(props.shield));

  if (props.isLoading) {
    return <Skeleton loading boxSize={ styles.boxSize } borderRadius="base" mr={ 2 }/>;
  }

  const fallbackElement = (
    <Box
      display="inline-flex"
      alignItems="center"
      justifyContent="center"
      width={ styles.boxSize }
      height={ styles.boxSize }
      backgroundColor="clusters"
      borderRadius="base"
      mr={ 2 }
      flexShrink={ 0 }
    >
      <IconSvg
        name="clusters"
        width="100%"
        height="100%"
        color="white"
        style={{ filter: 'brightness(0) invert(1)' }}
      />
    </Box>
  );

  const profileImageElement = (
    <Image
      width={ styles.boxSize }
      height={ styles.boxSize }
      borderRadius="base"
      mr={ 2 }
      flexShrink={ 0 }
      src={ `${ clustersFeature?.cdnUrl || '' }/profile-image/${ props.clusterName }` }
      alt={ `${ props.clusterName } profile` }
      fallback={ fallbackElement }
    />
  );

  const tooltipContent = (
    <>
      <Flex alignItems="center" textStyle="md">
        <Box
          display="inline-flex"
          alignItems="center"
          justifyContent="center"
          boxSize={ 5 }
          backgroundColor="clusters"
          borderRadius="sm"
          mr={ 2 }
        >
          <IconSvg name="clusters" boxSize={ 4 } color="white" style={{ filter: 'brightness(0) invert(1)' }}/>
        </Box>
        <div>
          <span>Clusters</span>
          <chakra.span color="text.secondary" whiteSpace="pre"> - Universal name service</chakra.span>
        </div>
      </Flex>
      <Text>
        Clusters provides unified naming across multiple blockchains including EVM, Solana, Bitcoin, and more.
        Manage all your wallet addresses under one human-readable name.
      </Text>
      <LinkToolkit
        href="https://clusters.xyz"
        display="inline-flex"
        alignItems="center"
        external
      >
        <IconSvg name="link" boxSize={ 5 } color="text.secondary" mr={ 2 }/>
        <span>Learn more about Clusters</span>
      </LinkToolkit>
    </>
  );

  return (
    <Tooltip
      content={ tooltipContent }
      variant="popover"
      positioning={{
        placement: 'bottom-start',
      }}
      contentProps={{
        maxW: { base: 'calc(100vw - 8px)', lg: '440px' },
        minW: '250px',
        w: 'fit-content',
        display: 'flex',
        flexDir: 'column',
        rowGap: 3,
        alignItems: 'flex-start',
      }}
      interactive
    >
      { profileImageElement }
    </Tooltip>
  );
};

type ContentProps = Omit<EntityBase.ContentBaseProps, 'text'> & Pick<EntityProps, 'clusterName'>;

const Content = chakra((props: ContentProps) => {
  const shouldShowTrailingSlash = !props.clusterName.includes('/');
  const displayName = shouldShowTrailingSlash ? `${ props.clusterName }/` : props.clusterName;

  return (
    <EntityBase.Content
      { ...props }
      text={ displayName }
      truncation="tail"
    />
  );
});

type CopyProps = Omit<EntityBase.CopyBaseProps, 'text'> & Pick<EntityProps, 'clusterName'>;

const Copy = (props: CopyProps) => {
  return (
    <EntityBase.Copy
      { ...props }
      text={ props.clusterName }
    />
  );
};

const Container = EntityBase.Container;

export interface EntityProps extends EntityBase.EntityBaseProps {
  clusterName: string;
}

const ClustersEntity = (props: EntityProps) => {
  const partsProps = distributeEntityProps(props);
  const content = <Content { ...partsProps.content }/>;

  return (
    <Container { ...partsProps.container }>
      <Icon { ...partsProps.icon }/>
      { props.noLink ? content : <Link { ...partsProps.link }>{ content }</Link> }
      <Copy { ...partsProps.copy }/>
    </Container>
  );
};

export default React.memo(chakra(ClustersEntity));

export {
  Container,
  Link,
  Icon,
  Content,
  Copy,
};
