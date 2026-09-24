import { chakra } from '@chakra-ui/react';
import React from 'react';

import type { TokenInstance } from 'types/api/token';

import { route } from 'nextjs/routes';

import { useMultichainContext } from 'lib/contexts/multichain';
import * as EntityBase from 'ui/shared/entities/base/components';
import NftMedia from 'ui/shared/nft/NftMedia';

import { distributeEntityProps, getIconProps } from '../base/utils';

const Container = EntityBase.Container;

type IconProps = EntityBase.IconBaseProps & {
  instance?: TokenInstance | null;
};

const ICON_MEDIA_TYPES = [ 'image' as const ];

const Icon = (props: IconProps) => {
  if (props.noIcon) {
    return null;
  }

  if (props.instance) {
    const styles = getIconProps({ ...props, variant: props.variant ?? 'heading' });
    const fallback = (
      <EntityBase.Icon
        { ...props }
        variant={ props.variant ?? 'heading' }
        name={ 'name' in props ? props.name : 'nft_shield' }
        marginRight={ 0 }
      />
    );

    return (
      <NftMedia
        data={ props.instance }
        isLoading={ props.isLoading }
        boxSize={ styles.boxSize }
        size="sm"
        allowedTypes={ ICON_MEDIA_TYPES }
        borderRadius="sm"
        flexShrink={ 0 }
        marginRight={ 2 }
        fallback={ fallback }
      />
    );
  }

  return (
    <EntityBase.Icon
      { ...props }
      variant="heading"
      name={ 'name' in props ? props.name : 'nft_shield' }
    />
  );
};

type LinkProps = EntityBase.LinkBaseProps & Pick<EntityProps, 'hash' | 'id'>;

const Link = chakra((props: LinkProps) => {
  const defaultHref = route(
    { pathname: '/token/[hash]/instance/[id]', query: { hash: props.hash, id: props.id } },
    { chain: props.chain, external: props.external },
  );

  return (
    <EntityBase.Link
      { ...props }
      href={ props.href ?? defaultHref }
    >
      { props.children }
    </EntityBase.Link>
  );
});

type ContentProps = Omit<EntityBase.ContentBaseProps, 'text'> & Pick<EntityProps, 'id'>;

const Content = chakra((props: ContentProps) => {
  return (
    <EntityBase.Content
      { ...props }
      text={ props.id }
      truncation="tail"
    />
  );
});

export interface EntityProps extends EntityBase.EntityBaseProps {
  hash: string;
  id: string;
  instance?: TokenInstance | null;
}

const NftEntity = (props: EntityProps) => {
  const multichainContext = useMultichainContext();
  const partsProps = distributeEntityProps(props, multichainContext);

  const content = <Content { ...partsProps.content }/>;

  return (
    <Container w="100%" { ...partsProps.container }>
      <Icon { ...partsProps.icon }/>
      { props.noLink ? content : <Link { ...partsProps.link }>{ content }</Link> }
    </Container>
  );
};

export default React.memo(chakra(NftEntity));

export {
  Container,
  Link,
  Icon,
  Content,
};
