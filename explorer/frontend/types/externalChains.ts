import type { EssentialDappsChainConfig } from 'types/client/marketplace';
import type { ClusterChainConfig } from 'types/multichain';

// minimal set of fields for external chains
export interface ExternalChain {
  id: string;
  name: string;
  logo?: string;
  explorer_url?: string;
  route_templates?: {
    tx?: string;
    address?: string;
    token?: string;
  };
}

export type ExternalChainExtended = ClusterChainConfig | EssentialDappsChainConfig;
