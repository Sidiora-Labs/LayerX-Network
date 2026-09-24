import type { IconName } from 'ui/shared/IconSvg';

export type DeFiDropdownItem = {
  text: string;
  icon?: IconName;
} & (
  { dappId: string; isEssentialDapp?: boolean; url?: never } |
  { url: string; dappId?: never; isEssentialDapp?: never }
);
