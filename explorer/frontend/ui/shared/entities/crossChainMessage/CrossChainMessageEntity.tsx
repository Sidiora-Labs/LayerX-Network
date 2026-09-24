import { chakra } from '@chakra-ui/react';
import React from 'react';

import { route } from 'nextjs/routes';

import * as EntityBase from 'ui/shared/entities/base/components';
import getChainTooltipText from 'ui/shared/externalChains/getChainTooltipText';

import { distributeEntityProps } from '../base/utils';

type LinkProps = EntityBase.LinkBaseProps & Pick<EntityProps, 'id'>;

const Link = chakra((props: LinkProps) => {
  const defaultHref = route(
    { pathname: '/cross-chain-tx/[id]', query: { id: props.id } },
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

type IconProps = EntityBase.IconBaseProps;

const Icon = ({ noIcon, ...props }: IconProps) => {
  // by default, we don't show the icon
  if (noIcon !== false) {
    return null;
  }

  const hint = (() => {
    if ('hint' in props) {
      return props.hint;
    }

    if (props.chain && props.shield !== false) {
      return getChainTooltipText(props.chain, 'Cross-chain transaction on ');
    }
  })();

  return (
    <EntityBase.Icon
      name="interop"
      { ...props }
      shield={ props.shield ?? (props.chain ? { src: props.chain.logo } : undefined) }
      hint={ hint }
    />
  );
};

type ContentProps = Omit<EntityBase.ContentBaseProps, 'text'> & Pick<EntityProps, 'id'>;

const Content = chakra((props: ContentProps) => {
  return (
    <EntityBase.Content
      truncation="constant"
      { ...props }
      text={ props.id }
    />
  );
});

type CopyProps = Omit<EntityBase.CopyBaseProps, 'text'> & Pick<EntityProps, 'id'>;

const Copy = (props: CopyProps) => {
  return (
    <EntityBase.Copy
      { ...props }
      text={ props.id }
      noCopy={ props.noCopy }
    />
  );
};

const Container = EntityBase.Container;

export interface EntityProps extends EntityBase.EntityBaseProps {
  id: string;
}

const CrossChainMessageEntity = (props: EntityProps) => {
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

export default React.memo(chakra(CrossChainMessageEntity));

export {
  Container,
  Link,
  Icon,
  Content,
  Copy,
};
