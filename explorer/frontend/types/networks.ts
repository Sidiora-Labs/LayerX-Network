import type { ArrayElement } from 'types/utils';

export const NETWORK_GROUPS = [ 'Mainnets', 'Testnets', 'Other' ] as const;
export type NetworkGroup = ArrayElement<typeof NETWORK_GROUPS>;

export interface FeaturedNetwork {
  title: string;
  url: string;
  group: NetworkGroup;
  icon?: string;
  isActive?: boolean;
  invertIconInDarkMode?: boolean;
}

export interface NetworkExplorer {
  logo?: string;
  title: string;
  baseUrl: string;
  paths: {
    tx?: string;
    address?: string;
    token?: string;
    block?: string;
    blob?: string;
  };
}

export type NetworkVerificationTypeEnvs = 'mining' | 'validation' | 'fee reception';
export type NetworkVerificationTypeComputed = 'posting' | 'sequencing';
export type NetworkVerificationType = NetworkVerificationTypeEnvs | NetworkVerificationTypeComputed;
