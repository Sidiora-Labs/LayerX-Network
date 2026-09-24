import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, resolve as resolvePath } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Worker } from 'node:worker_threads';
import { delay } from 'es-toolkit';

import { ClusterChainConfig } from 'types/multichain';
import appConfig from 'configs/app';

const currentFilePath = fileURLToPath(import.meta.url);
const currentDir = dirname(currentFilePath);

// const EXPLORER_URLS = [
//   'https://optimism.blockscout.com',
//   'https://base.blockscout.com',
//   'https://arbitrum.blockscout.com',
//   'https://unichain.blockscout.com',
//   'https://explorer.redstone.xyz',
//   'https://matchscan.io'
//   // 'https://optimism-interop-alpha-0.blockscout.com',
//   // 'https://optimism-interop-alpha-1.blockscout.com',
// ];

function getSlug(chainName: string) {
  return chainName.toLowerCase().replace(/ /g, '-').replace(/[^a-z0-9-]/g, '');
}

async function getChainscoutInfo(chainIds: Array<string>) {
  const controller = new AbortController();
  const timeout = setTimeout(() => {
    controller.abort(`Request to Chainscout API timed out`);
  }, 30_000);

  try {
    const response = await fetch(`https://chains.blockscout.com/api/chains?chain_ids=${ chainIds.join(',') }`, { signal: controller.signal });
    if (!response.ok) {
      throw new Error(`Failed to fetch chains info from Chainscout API`);
    }
    const chainsInfo = await response.json() as Record<string, { explorers: [ { url: string } ], logo: string }>;
  
    return chainIds.map((chainId) => ({
      id: chainId,
      logoUrl: chainsInfo[chainId]?.logo,
    }))
  } catch (error) {
    throw error;
  } finally {
    clearTimeout(timeout);
  }
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

async function getExplorerUrls() {
  // return EXPLORER_URLS;
  try {
    const basePath = `/api/v1/clusters/${ process.env.NEXT_PUBLIC_MULTICHAIN_CLUSTER }/chains`;
    const url = new URL(basePath, process.env.NEXT_PUBLIC_MULTICHAIN_AGGREGATOR_API_HOST);

    const response = await fetch(url.toString());
    const data = await response.json();

    console.log(`💡 Found ${ data.items.length } chains in cluster.`);

    return data.items.map((item: { explorer_url: string }) => item.explorer_url);
  } catch (error) {
    return [];
  }
}

async function run() {
  try {
    if (!process.env.NEXT_PUBLIC_MULTICHAIN_AGGREGATOR_API_HOST) {
      console.log('ℹ️  NEXT_PUBLIC_MULTICHAIN_AGGREGATOR_API_HOST is not set, skipping multichain config generation\n');
      return;
    }

    console.log('🌀 Generating multichain config...');

    const explorerUrls = await getExplorerUrls();

    if (!explorerUrls.length) {
      throw new Error('No chains found in the cluster.');
    }

    const configs: Array<typeof appConfig> = [];
    for (const url of explorerUrls) {
      const chainConfig = (await computeChainConfig(url)) as typeof appConfig | undefined;
      if (!chainConfig) {
        throw new Error(`Failed to fetch chain config for ${ url }`);
      }
      configs.push(chainConfig);
    }

    const chainscoutInfo = await getChainscoutInfo(
      configs
      .map((config) => config.chain.id)
      .filter((chainId) => chainId !== undefined)
    );

    const config = {
      chains: configs.map((config, index) => {
        const chainId = config.chain.id;
        const chainName = (config as { chain: { name: string } })?.chain?.name ?? `Chain ${ chainId }`;
        return {
          id: chainId || '',
          name: chainName,
          logo: chainscoutInfo.find((chain) => chain.id === chainId)?.logoUrl,
          explorer_url: explorerUrls[index],
          slug: getSlug(chainName),
          app_config: config,
        } satisfies ClusterChainConfig;
      }),
    };

    const outputDir = resolvePath(currentDir, '../../../../public/assets/multichain');
    mkdirSync(outputDir, { recursive: true });

    const outputPathJson = resolvePath(outputDir, 'config.json');
    writeFileSync(outputPathJson, JSON.stringify(config, null, 2));

    const outputPathJs = resolvePath(outputDir, 'config.js');
    writeFileSync(outputPathJs, `window.__multichainConfig = ${ JSON.stringify(config) };`);

    console.log('👍 Done!\n');
  } catch (error) {
    console.error('🚨 Error generating multichain config:', error);
    console.log('\n');
    process.exit(1);
  }
}

run();
