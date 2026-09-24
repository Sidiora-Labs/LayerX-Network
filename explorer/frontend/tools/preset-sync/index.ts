import fs from 'fs';
import path from 'path';

/* eslint-disable no-console */
const PRESETS = {
  arbitrum: 'https://arbitrum.blockscout.com',
  arbitrum_sepolia: 'https://arbitrum-sepolia.blockscout.com',
  base: 'https://base.blockscout.com',
  blackfort_testnet: 'https://blackfort-testnet.blockscout.com',
  celo: 'https://celo.blockscout.com',
  celo_sepolia: 'https://celo-sepolia.blockscout.com',
  eth: 'https://eth.blockscout.com',
  eth_sepolia: 'https://eth-sepolia.blockscout.com',
  filecoin: 'https://filecoin.blockscout.com',
  garnet: 'https://explorer.garnetchain.com',
  gnosis: 'https://gnosis.blockscout.com',
  immutable: 'https://explorer.immutable.com',
  mega_eth: 'https://megaeth.blockscout.com',
  mekong: 'https://mekong.blockscout.com',
  multichain_dev: 'https://superchain.k8s-dev.blockscout.com',
  // multichain_prod: 'https://multichain.blockscout.com',
  neon_devnet: 'https://neon-devnet.blockscout.com',
  numine: 'https://numine.blockscout.com',
  optimism: 'https://optimism.blockscout.com',
  optimism_sepolia: 'https://optimism-sepolia.blockscout.com',
  polygon: 'https://polygon.blockscout.com',
  rari_testnet: 'https://rari-testnet.cloud.blockscout.com',
  rootstock: 'https://rootstock.blockscout.com',
  rootstock_testnet: 'https://rootstock-testnet.blockscout.com',
  scroll_sepolia: 'https://scroll-sepolia.blockscout.com',
  shibarium: 'https://www.shibariumscan.io',
  stability_testnet: 'https://stability-testnet.blockscout.com',
  tac: 'https://explorer.tac.build',
  tac_spb: 'https://spb.explorer.tac.build',
  zetachain: 'https://zetascan.com',
  zetachain_testnet: 'https://testnet.zetascan.com',
  zkevm: 'https://polygon-cdk-stavanger.blockscout.com',
  zksync: 'https://zksync.blockscout.com',
  zilliqa: 'https://zilliqa.blockscout.com',
  zora: 'https://explorer.zora.energy',
  // main === staging
  main: 'https://eth-sepolia.k8s-dev.blockscout.com',
};

const LOCAL_ENVS = {
  NEXT_PUBLIC_APP_PROTOCOL: 'http',
  NEXT_PUBLIC_APP_HOST: 'localhost',
  NEXT_PUBLIC_APP_PORT: '3000',
  NEXT_PUBLIC_APP_ENV: 'development',
  NEXT_PUBLIC_API_WEBSOCKET_PROTOCOL: 'ws',
};

const IGNORED_ENVS = [
  'NEXT_PUBLIC_GIT_COMMIT_SHA',
  'NEXT_PUBLIC_GIT_TAG',
  'NEXT_PUBLIC_ICON_SPRITE_HASH',

  // DEPRECATED
  'NEXT_PUBLIC_AUTH0_CLIENT_ID',
  'NEXT_PUBLIC_AUTH_URL',
  'NEXT_PUBLIC_LOGOUT_URL',
  'NEXT_PUBLIC_SENTRY_DSN',
  'SENTRY_CSP_REPORT_URI',
  'NEXT_PUBLIC_SENTRY_ENABLE_TRACING',
  'NEXT_PUBLIC_ROLLUP_PARENT_CHAIN_NAME',
  'NEXT_PUBLIC_RE_CAPTCHA_V3_APP_SITE_KEY',
  'NEXT_PUBLIC_HOMEPAGE_PLATE_TEXT_COLOR',
  'NEXT_PUBLIC_HOMEPAGE_PLATE_BACKGROUND',
  'NEXT_PUBLIC_VIEWS_CONTRACT_LANGUAGE_FILTERS',

  'NEXT_PUBLIC_API_SPEC_URL',
  'NEXT_PUBLIC_ROLLUP_L1_BASE_URL',
];

function parseScriptArgs() {
  const args = process.argv.slice(2);

  const result: Record<string, string> = {};
  args.forEach(arg => {
    const [ key, value ] = arg.split('=');
    if (key && value) {
      result[key.replace(/^--/, '') as keyof typeof result] = value;
    }
  });

  return result;
}

function getSecretEnvsList() {
  const fileContent = fs.readFileSync(path.resolve(__dirname, '../../.env.example'), 'utf8');
  const result = fileContent.split('\n').map((line) => line.split('=')[0]);
  return result;
}

function updateFileContent(envsEntries: Array<[ string, string ]>, presetId: keyof typeof PRESETS) {
  const presetEnvsContent = envsEntries
    .map(([ key, value ]) => `${ key }=${ value }`)
    .join('\n');

  const chainName = envsEntries.find(([ key ]) => key === 'NEXT_PUBLIC_NETWORK_NAME')?.[1] ?? 'Unknown';

  const explorerUrl = PRESETS[presetId];

  const localEnvsContent = Object.entries(LOCAL_ENVS)
    .map(([ key, value ]) => `${ key }=${ value }`)
    .join('\n');

  const content =
    `# Set of ENVs for ${ chainName } network explorer\n` +
    '# ' + explorerUrl + '\n' +
    `# This is an auto-generated file. To update all values, run "yarn dev:preset:sync --name=${ presetId }"\n` +
    '\n' +
    '# Local ENVs\n' +
    localEnvsContent + '\n' +
    '\n' +
    '# Instance ENVs\n' +
    presetEnvsContent;

  fs.writeFileSync(path.resolve(__dirname, `../../configs/envs/.env.${ presetId }`), content);
}

async function updatePresetFile(presetId: keyof typeof PRESETS) {
  const secretEnvs = getSecretEnvsList();

  const instanceUrl = PRESETS[presetId];
  const response = await fetch(`${ instanceUrl }/node-api/config`);
  const instanceConfig = await response.json() as Record<'envs', Record<string, string>>;

  const ignoredEnvs = [
    ...Object.keys(LOCAL_ENVS),
    ...IGNORED_ENVS,
    ...secretEnvs,
  ];

  const presetEnvsEntries = Object.entries(instanceConfig.envs)
    .filter(([ key ]) => !ignoredEnvs.includes(key));

  updateFileContent(presetEnvsEntries, presetId);
}

async function run() {
  const args = parseScriptArgs();
  if (!args.name) {
    console.log('🚨 No "--name" argument is provided. Exiting...');
    return;
  }

  const name = args.name;

  if (name === 'all') {
    console.log(`🌀 Syncing all presets configuration files...`);

    for (const presetId in PRESETS) {
      await updatePresetFile(presetId as keyof typeof PRESETS);
      console.log(`  - [v] "${ presetId }" is ready`);
    }

    console.log(`✅ Done!`);

    return;
  }

  const presetId = name as keyof typeof PRESETS;

  const instanceUrl = PRESETS[presetId];
  if (!instanceUrl) {
    console.log(`🚨 No preset with name "${ presetId }" found. Exiting...`);
    return;
  }

  console.log(`🌀 Syncing preset configuration file...`);

  await updatePresetFile(presetId);

  console.log(`✅ Done!`);
}

run();
