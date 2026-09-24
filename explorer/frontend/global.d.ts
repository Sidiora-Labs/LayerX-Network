import type { EssentialDappsChainConfig } from 'types/client/marketplace';
import type { MultichainConfig } from 'types/multichain';
import type { WalletProvider } from 'types/web3';
import 'vitest-fetch-mock';

type CPreferences = {
  zone: string;
  width: string;
  height: string;
};

declare global {
  export interface Window {
    ethereum?: WalletProvider | undefined;
    coinzilla_display: Array<CPreferences>;
    sevioads: Array<Array<Record<string, string>>> | undefined;
    ga?: {
      getAll: () => Array<{ get: (prop: string) => string }>;
    };
    AdButler: {
      ads: Array<unknown>;
      register: (...args: unknown) => void;
    };
    abkw: string;
    __envs: Record<string, string>;
    __multichainConfig?: MultichainConfig;
    __essentialDappsChains?: { chains: Array<EssentialDappsChainConfig> };
  }

  namespace NodeJS {
    interface ProcessEnv {
      NODE_ENV: 'development' | 'production';
    }
  }
}

export {};
