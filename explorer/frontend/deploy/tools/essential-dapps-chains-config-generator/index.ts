/* eslint-disable no-console */
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, resolve as resolvePath } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Worker } from 'node:worker_threads';
import * as viemChains from 'viem/chains';
import { pick, uniq, delay } from 'es-toolkit';

import { EssentialDappsConfig } from 'types/client/marketplace';
import { getEnvValue, parseEnvJson } from 'configs/app/utils';
import currentChainConfig from 'configs/app';
import appConfig from 'configs/app';
import { EssentialDappsChainConfig } from 'types/client/marketplace';

const currentFilePath = fileURLToPath(import.meta.url);
const currentDir = dirname(currentFilePath);

async function getChainscoutInfo(externalChainIds: Array<string>, currentChainId: string | undefined) {
  
  const controller = new AbortController();
  const timeout = setTimeout(() => {
    controller.abort(`Request to Chainscout API timed out`);
  }, 30_000);

  try {
    const response = await fetch(
      `https://chains.blockscout.com/api/chains?chain_ids=${ [currentChainId, ...externalChainIds].filter(Boolean).join(',') }`,
      { signal: controller.signal }
    );
    if (!response.ok) {
      throw new Error(`Failed to fetch chains info from Chainscout API`);
    }
    const chainsInfo = await response.json() as Record<string, { explorers: [ { url: string } ], logo: string }>;
  
    return {
      externals: externalChainIds.map((chainId) => ({
        id: chainId,
        explorerUrl: chainsInfo[chainId]?.explorers[0]?.url,
        logoUrl: chainsInfo[chainId]?.logo,
      })),
      current: currentChainId ? {
        id: currentChainId,
        explorerUrl: chainsInfo[currentChainId]?.explorers[0]?.url,
        logoUrl: chainsInfo[currentChainId]?.logo,
      } : undefined,
    }
    
  } catch (error) {
    throw error;
  } finally {
    clearTimeout(timeout);
  }
}

function trimChainConfig(config: typeof appConfig, logoUrl: string | undefined) {
  return {
    ...pick(config, [ 'app', 'chain' ]),
    apis: pick(config.apis || {}, [ 'general' ]),
  };
}

async function computeChainConfig(url: string): Promise<unknown> {
  const workerPath = resolvePath(currentDir, 'worker.js');

  const worker = new Worker(workerPath, {
    workerData: { url },
    env: {} // Start with empty environment
  });
  const controller = new AbortController();

  const configPromise = new Promise((resolve, reject) => {
    worker.on('message', (config) => {
      resolve(config);
    });

    worker.on('error', (error) => {
      console.error('Worker error:', error);
      reject(error);
    });

    worker.on('exit', (code) => {
      controller.abort();
      reject(new Error(`Worker stopped with exit code ${ code }`));
    });
  });

  return Promise.race([
    configPromise,
    delay(30_000, { signal: controller.signal })
  ]).finally(() => {
    worker.terminate();
  })
}

async function run() {
  try {
    if (!process.env.NEXT_PUBLIC_MARKETPLACE_ESSENTIAL_DAPPS_CONFIG) {
      console.log('ℹ️  NEXT_PUBLIC_MARKETPLACE_ESSENTIAL_DAPPS_CONFIG is not set, skipping essential dapps chains config generation\n');
      return;
    }

    console.log('🌀 Generating essential dapps chains config...');

    const featureConfig = parseEnvJson<EssentialDappsConfig>(getEnvValue('NEXT_PUBLIC_MARKETPLACE_ESSENTIAL_DAPPS_CONFIG'));
    const enabledChains = uniq([
      ...Object.values(featureConfig?.swap?.chains ?? []),
      ...Object.values(featureConfig?.revoke?.chains ?? []),
      ...Object.values(featureConfig?.multisend?.chains ?? []),
    ].filter((chain) => chain !== currentChainConfig.chain.id))

    if (enabledChains.length === 0) {
      console.log('⚠️  No chains are enabled. Skipping essential dapps chains config generation.\n');
      return;
    }

    const chainscoutInfo = await getChainscoutInfo(enabledChains, currentChainConfig.chain.id);
    const chainsWithoutUrl = chainscoutInfo.externals.filter(({ explorerUrl }) => !explorerUrl);

    if (chainsWithoutUrl.length > 0) {
      console.log(`⚠️  For the following chains explorer url was not found: ${ chainsWithoutUrl.map(({ id }) => id).join(', ') }. Therefore, they will not be enabled.`);
    }
    const explorerUrls = chainscoutInfo.externals.map(({ explorerUrl }) => explorerUrl).filter(Boolean);
    console.log(`ℹ️  For ${ explorerUrls.length } chains explorer url was found in static config. Fetching parameters for each chain...`);

    const chainConfigs: Array<typeof appConfig> = [];

    for (const explorerUrl of explorerUrls) {
      const chainConfig = (await computeChainConfig(explorerUrl)) as typeof appConfig | undefined;
      if (!chainConfig) {
        throw new Error(`Failed to fetch chain config for ${ explorerUrl }`);
      }
      chainConfigs.push(chainConfig);
    }

    const result = {
      chains: [ currentChainConfig, ...chainConfigs ].map((config) => {
        const chainId = config.chain.id;
        const chainInfo = [...chainscoutInfo.externals, chainscoutInfo.current].find((chain) => chain?.id === chainId);
        const logoUrl = [...chainscoutInfo.externals, chainscoutInfo.current].find((chain) => chain?.id === config.chain.id)?.logoUrl;
        const chainName = (config as { chain: { name: string } })?.chain?.name ?? `Chain ${ chainId }`;
        return {
          id: chainId || '',
          name: chainName,
          logo: chainInfo?.logoUrl,
          explorer_url: chainInfo?.explorerUrl || '',
          app_config: trimChainConfig(config, logoUrl),
          contracts: Object.values(viemChains).find(({ id }) => id === Number(config.chain.id))?.contracts
        } satisfies EssentialDappsChainConfig;
      }),
    };

    const outputDir = resolvePath(currentDir, '../../../../public/assets/essential-dapps');
    mkdirSync(outputDir, { recursive: true });

    const outputPathJson = resolvePath(outputDir, 'chains.json');
    writeFileSync(outputPathJson, JSON.stringify(result, null, 2));

    const outputPathJs = resolvePath(outputDir, 'chains.js');
    writeFileSync(outputPathJs, `window.__essentialDappsChains = ${ JSON.stringify(result) };`);

    console.log('👍 Done!\n');
  } catch (error) {
    console.error('🚨 Error generating essential dapps chains config:', error);
    console.log('\n');
    process.exit(1);
  }
}

run();
