import type { ArrayElement } from 'types/utils';

import type { IconName } from 'ui/shared/IconSvg';

export const SUPPORTED_WALLETS = [
  'metamask',
  'coinbase',
  'token_pocket',
  'rabby',
  'okx',
  'trust',
] as const;

export type WalletType = ArrayElement<typeof SUPPORTED_WALLETS>;

export interface WalletInfo {
  name: string;
  icon: IconName;
  color: string;
}
