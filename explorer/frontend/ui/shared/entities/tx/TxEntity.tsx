import { chakra } from '@chakra-ui/react';
import React from 'react';

import { route } from 'nextjs/routes';

import config from 'configs/app';
import { useMultichainContext } from 'lib/contexts/multichain';
import * as EntityBase from 'ui/shared/entities/base/components';
import getChainTooltipText from 'ui/shared/externalChains/getChainTooltipText';

import { distributeEntityProps } from '../base/utils';

type LinkProps = EntityBase.LinkBaseProps & Pick<EntityProps, 'hash'>;

const Link = chakra((props: LinkProps) => {
  const defaultHref = route(
    { pathname: '/tx/[hash]', query: { hash: props.hash } },
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

type IconProps = EntityBase.IconBaseProps & Pick<EntityProps, 'isPendingUpdate'>;

const Icon = (props: IconProps) => {
  const isPendingUpdate = props.isPendingUpdate && config.UI.views.block.pendingUpdateAlertEnabled;

  const name = (() => {
    if ('name' in props) {
      return props.name;
    }

    return isPendingUpdate ? 'status/warning' : 'transactions';
  })();

  const hint = (() => {
    if ('hint' in props) {
      return props.hint;
    }

    if (props.chain && props.shield !== false) {
      return getChainTooltipText(props.chain, 'Transaction on ');
    }

    return isPendingUpdate ?
      'This transaction is part of a block that is being re-synced. Details may be incomplete until the update is finished.' :
      undefined;
  })();

  const shield = (() => {
    if ('shield' in props) {
      return props.shield;
    }

    if (props.chain) {
      return props.chain.logo ? { src: props.chain.logo } : { name: 'networks/icon-placeholder' as const };
    }
  })();

  return (
    <EntityBase.Icon
      { ...props }
      name={ name }
      shield={ shield }
      hint={ hint }
    />
  );
};

type ContentProps = Omit<EntityBase.ContentBaseProps, 'text'> & Pick<EntityProps, 'hash' | 'text'>;

const Content = chakra((props: ContentProps) => {
  return (
    <EntityBase.Content
      { ...props }
      text={ props.text ?? props.hash }
    />
  );
});

type CopyProps = Omit<EntityBase.CopyBaseProps, 'text'> & Pick<EntityProps, 'hash'>;

const Copy = (props: CopyProps) => {
  return (
    <EntityBase.Copy
      { ...props }
      text={ props.hash }
      noCopy={ props.noCopy }
    />
  );
};

const Container = EntityBase.Container;

export interface EntityProps extends EntityBase.EntityBaseProps {
  hash: string;
  text?: string;
  isPendingUpdate?: boolean;
}

const TxEntity = (props: EntityProps) => {
  const multichainContext = useMultichainContext();
  const partsProps = distributeEntityProps(props, multichainContext);

  const content = <Content { ...partsProps.content }/>;

  return (
    <Container { ...partsProps.container }>
      <Icon { ...partsProps.icon } isPendingUpdate={ props.isPendingUpdate }/>
      { props.noLink ? content : <Link { ...partsProps.link }>{ content }</Link> }
      <Copy { ...partsProps.copy }/>
    </Container>
  );
};

export default React.memo(chakra(TxEntity));

export {
  Container,
  Link,
  Icon,
  Content,
  Copy,
};
