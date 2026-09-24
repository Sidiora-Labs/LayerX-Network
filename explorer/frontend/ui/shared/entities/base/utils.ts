import type { TMultichainContext } from 'lib/contexts/multichain';

import type { IconBaseProps, EntityBaseProps } from './components';

export function getIconProps(props: IconBaseProps, withShield: boolean = false) {
  const variant = props.variant ?? 'content';

  switch (variant) {
    case 'content':
    case 'subheading': {
      return {
        boxSize: props.boxSize ?? '20px', // for tables, lists, regular content and page subheadings
        marginRight: props.marginRight ?? props.mr ?? (withShield ? '18px' : '8px'),
      };
    }
    case 'heading': {
      return {
        boxSize: props.boxSize ?? '30px', // for page headings
        marginRight: props.marginRight ?? props.mr ?? (withShield ? '14px' : '8px'),
      };
    }
  }
}

export function getContentProps(variant: EntityBaseProps['variant'] = 'content') {
  switch (variant) {
    // currently, there could be only icon in the heading variant
    // and for the content variant, fontStyle is set in the consumer component
    case 'subheading': {
      return {
        textStyle: { base: 'heading.sm', lg: 'heading.md' },
      };
    }
  }
}

export function distributeEntityProps<Props extends EntityBaseProps>(props: Props, multichainContext?: TMultichainContext | null) {
  const { className, onClick, icon, noIcon, link, chain, ...mainProps } = props;
  const { variant, ...restProps } = mainProps;

  return {
    container: { className },
    // For entities within the multichain views, we decided not to highlight the chain in the entity icon unless the chain data is passed manually via props.
    // This does not apply to the links. If the links are within the multichain views, they should lead to chain-specific pages.
    icon: { ...mainProps, ...icon, chain, noIcon },
    link: { ...restProps, ...link, onClick, chain: chain ?? multichainContext?.chain },
    content: mainProps,
    symbol: restProps,
    copy: restProps,
  };
}
