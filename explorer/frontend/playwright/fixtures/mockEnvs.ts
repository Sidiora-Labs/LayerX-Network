/* eslint-disable max-len */
import type { TestFixture, Page } from '@playwright/test';

export type MockEnvsFixture = (envs: Array<[string, string]>) => Promise<void>;

const fixture: TestFixture<MockEnvsFixture, { page: Page }> = async({ page }, use) => {
  await use(async(envs) => {
    for (const [ name, value ] of envs) {
      await page.evaluate(({ name, value }) => {
        window.localStorage.setItem(name, value);
      }, { name, value });
    }
  });
};

export default fixture;

export const ENVS_MAP: Record<string, Array<[string, string]>> = {
  optimisticRollup: [
    [ 'NEXT_PUBLIC_ROLLUP_TYPE', 'optimistic' ],
    [ 'NEXT_PUBLIC_ROLLUP_PARENT_CHAIN', '{"id":11155111,"name":"Sepolia","baseUrl":"https://localhost:3101","rpcUrls":["https://localhost:3101"],"currency":{"name":"Ether","symbol":"ETH","decimals":18},"isTestnet":true}' ],
    [ 'NEXT_PUBLIC_ROLLUP_L2_WITHDRAWAL_URL', 'https://localhost:3102' ],
    [ 'NEXT_PUBLIC_FAULT_PROOF_ENABLED', 'true' ],
    [ 'NEXT_PUBLIC_ROLLUP_DA_CELESTIA_CELENIUM_URL', 'https://mocha.celenium.io/blob' ],
  ],
  arbitrumRollup: [
    [ 'NEXT_PUBLIC_ROLLUP_TYPE', 'arbitrum' ],
    [ 'NEXT_PUBLIC_ROLLUP_PARENT_CHAIN', '{"name":"DuckChain","baseUrl":"https://localhost:3101"}' ],
    [ 'NEXT_PUBLIC_ROLLUP_DA_CELESTIA_NAMESPACE', '0x1234' ],
    [ 'NEXT_PUBLIC_ROLLUP_DA_CELESTIA_CELENIUM_URL', 'https://mocha.celenium.io/blob' ],
  ],
  shibariumRollup: [
    [ 'NEXT_PUBLIC_ROLLUP_TYPE', 'shibarium' ],
    [ 'NEXT_PUBLIC_ROLLUP_PARENT_CHAIN', '{"baseUrl":"https://localhost:3101"}' ],
  ],
  zkEvmRollup: [
    [ 'NEXT_PUBLIC_ROLLUP_TYPE', 'zkEvm' ],
    [ 'NEXT_PUBLIC_ROLLUP_PARENT_CHAIN', '{"baseUrl":"https://localhost:3101"}' ],
  ],
  zkSyncRollup: [
    [ 'NEXT_PUBLIC_ROLLUP_TYPE', 'zkSync' ],
    [ 'NEXT_PUBLIC_ROLLUP_PARENT_CHAIN', '{"baseUrl":"https://localhost:3101"}' ],
    [ 'NEXT_PUBLIC_VIEWS_CONTRACT_EXTRA_VERIFICATION_METHODS', 'none' ],
  ],
  scrollRollup: [
    [ 'NEXT_PUBLIC_ROLLUP_TYPE', 'scroll' ],
    [ 'NEXT_PUBLIC_ROLLUP_PARENT_CHAIN', '{"baseUrl":"https://localhost:3101"}' ],
  ],
  bridgedTokens: [
    [ 'NEXT_PUBLIC_BRIDGED_TOKENS_CHAINS', '[{"id":"1","title":"Ethereum","short_title":"ETH","base_url":"https://eth.blockscout.com/token/"},{"id":"56","title":"Binance Smart Chain","short_title":"BSC","base_url":"https://bscscan.com/token/"},{"id":"99","title":"POA","short_title":"POA","base_url":"https://blockscout.com/poa/core/token/"}]' ],
    [ 'NEXT_PUBLIC_BRIDGED_TOKENS_BRIDGES', '[{"type":"omni","title":"OmniBridge","short_title":"OMNI"},{"type":"amb","title":"Arbitrary Message Bridge","short_title":"AMB"}]' ],
  ],
  userOps: [
    [ 'NEXT_PUBLIC_HAS_USER_OPS', 'true' ],
    [ 'NEXT_PUBLIC_USER_OPS_INDEXER_API_HOST', 'http://localhost:3110' ],
  ],
  hasContractAuditReports: [
    [ 'NEXT_PUBLIC_HAS_CONTRACT_AUDIT_REPORTS', 'true' ],
  ],
  blockHiddenFields: [
    [ 'NEXT_PUBLIC_VIEWS_BLOCK_HIDDEN_FIELDS', '["burnt_fees", "total_reward", "nonce"]' ],
  ],
  stabilityEnvs: [
    [ 'NEXT_PUBLIC_VIEWS_ADDRESS_HIDDEN_VIEWS', '["top_accounts"]' ],
    [ 'NEXT_PUBLIC_VIEWS_TX_HIDDEN_FIELDS', '["value","fee_currency","gas_price","gas_fees","burnt_fees"]' ],
    [ 'NEXT_PUBLIC_VIEWS_TX_ADDITIONAL_FIELDS', '["fee_per_gas"]' ],
  ],
  beaconChain: [
    [ 'NEXT_PUBLIC_HAS_BEACON_CHAIN', 'true' ],
    [ 'NEXT_PUBLIC_BEACON_CHAIN_VALIDATOR_URL_TEMPLATE', 'https://beaconcha.in/validator/{pk}' ],
  ],
  txInterpretation: [
    [ 'NEXT_PUBLIC_TRANSACTION_INTERPRETATION_PROVIDER', 'blockscout' ],
  ],
  noWalletClient: [
    [ 'NEXT_PUBLIC_WALLET_CONNECT_PROJECT_ID', '' ],
  ],
  noAccount: [
    [ 'NEXT_PUBLIC_IS_ACCOUNT_SUPPORTED', 'false' ],
  ],
  noNftMarketplaces: [
    [ 'NEXT_PUBLIC_VIEWS_NFT_MARKETPLACES', '' ],
  ],
  navigationHighlightedRoutes: [
    [ 'NEXT_PUBLIC_NAVIGATION_HIGHLIGHTED_ROUTES', '["/blocks", "/apps"]' ],
  ],
  dataAvailability: [
    [ 'NEXT_PUBLIC_DATA_AVAILABILITY_ENABLED', 'true' ],
  ],
  nameService: [
    [ 'NEXT_PUBLIC_NAME_SERVICE_API_HOST', 'http://localhost:3008' ],
    [ 'NEXT_PUBLIC_NAME_SERVICE_PROTOCOLS', '["ens","duck"]' ],
  ],
  rewardsService: [
    [ 'NEXT_PUBLIC_REWARDS_SERVICE_API_HOST', 'http://localhost:3009' ],
  ],
  addressBech32Format: [
    [ 'NEXT_PUBLIC_ADDRESS_FORMAT', '["bech32","base16"]' ],
    [ 'NEXT_PUBLIC_VIEWS_ADDRESS_BECH_32_PREFIX', 'tom' ],
  ],
  externalTxs: [
    [ 'NEXT_PUBLIC_TX_EXTERNAL_TRANSACTIONS_CONFIG', '{"chain_name": "Solana", "chain_logo_url": "http://example.url", "explorer_url_template": "https://scan.io/tx/{hash}"}' ],
  ],
  interop: [
    [ 'NEXT_PUBLIC_ROLLUP_TYPE', 'optimistic' ],
    [ 'NEXT_PUBLIC_ROLLUP_PARENT_CHAIN', '{"baseUrl":"https://localhost:3101"}' ],
    [ 'NEXT_PUBLIC_ROLLUP_L2_WITHDRAWAL_URL', 'https://localhost:3102' ],
    [ 'NEXT_PUBLIC_INTEROP_ENABLED', 'true' ],
  ],
  tac: [
    [ 'NEXT_PUBLIC_TAC_OPERATION_LIFECYCLE_API_HOST', 'http://localhost:3100' ],
    [ 'NEXT_PUBLIC_TAC_TON_EXPLORER_URL', 'https://testnet.tonviewer.com' ],
  ],
  celo: [
    [ 'NEXT_PUBLIC_CELO_ENABLED', 'true' ],
  ],
  multichain: [
    [ 'NEXT_PUBLIC_MULTICHAIN_ENABLED', 'true' ],
    [ 'NEXT_PUBLIC_MULTICHAIN_CLUSTER', 'test' ],
    [ 'NEXT_PUBLIC_MULTICHAIN_AGGREGATOR_API_HOST', 'http://localhost:3012' ],
    [ 'NEXT_PUBLIC_MULTICHAIN_STATS_API_HOST', 'http://localhost:3013' ],
  ],
  clusters: [
    [ 'NEXT_PUBLIC_CLUSTERS_API_HOST', 'https://api.clusters.xyz' ],
  ],
  zetaChain: [
    [ 'NEXT_PUBLIC_ZETACHAIN_SERVICE_API_HOST', 'http://localhost:3111' ],
    [ 'NEXT_PUBLIC_ZETACHAIN_SERVICE_CHAINS_CONFIG_URL', 'http://localhost:3000/zeta-config.json' ],
    [ 'NEXT_PUBLIC_ZETACHAIN_EXTERNAL_SEARCH_CONFIG', '[{"regex":"^0x[0-9a-fA-F]{64}$","template":"https://example.com/tx/{hash}","name":"Cosmos SDK style transaction"}]' ],
  ],
  navigationPromoBannerText: [
    [ 'NEXT_PUBLIC_NAVIGATION_PROMO_BANNER_CONFIG', '{"img_url": "http://localhost:3000/image.svg", "text": "Try the DUCK!", "bg_color": {"light": "rgb(150, 211, 255)", "dark": "rgb(68, 51, 122)"}, "text_color": {"light": "rgb(69, 69, 69)", "dark": "rgb(233, 216, 253)"}, "link_url": "https://example.com"}' ],
  ],
  navigationPromoBannerImage: [
    [ 'NEXT_PUBLIC_NAVIGATION_PROMO_BANNER_CONFIG', '{"img_url": {"small": "http://localhost:3000/image_s.jpg", "large": "http://localhost:3000/image_md.jpg"}, "link_url": "https://example.com"}' ],
  ],
  colorThemeOverrides: [
    [ 'NEXT_PUBLIC_COLOR_THEME_OVERRIDES', '{"bg":{"primary":{"_light":{"value":"rgba(254,253,253)"},"_dark":{"value":"rgba(25,25,26)"}}},"text":{"primary":{"_light":{"value":"rgba(16,17,18,0.80)"},"_dark":{"value":"rgba(222,217,217)"}},"secondary":{"_light":{"value":"rgba(138,136,136)"},"_dark":{"value":"rgba(133,133,133)"}}},"hover":{"_light":{"value":"rgba(104,200,158)"},"_dark":{"value":"rgba(104,200,158)"}},"selected":{"control":{"text":{"_light":{"value":"rgba(25,25,26)"},"_dark":{"value":"rgba(247,250,252)"}},"bg":{"_light":{"value":"rgba(242,239,239)"},"_dark":{"value":"rgba(255,255,255,0.06)"}}},"option":{"bg":{"_light":{"value":"rgba(84,75,75)"},"_dark":{"value":"rgba(87,87,87)"}}}},"icon":{"primary":{"_light":{"value":"rgba(138,136,136)"},"_dark":{"value":"rgba(133,133,133)"}},"secondary":{"_light":{"value":"rgba(176,176,176)"},"_dark":{"value":"rgba(105,103,103)"}}},"button":{"primary":{"_light":{"value":"rgba(105,103,103)"},"_dark":{"value":"rgba(133,133,133)"}}},"link":{"primary":{"_light":{"value":"rgba(57,146,108)"},"_dark":{"value":"rgba(57,146,108)"}}},"graph":{"line":{"_light":{"value":"rgba(105,103,103)"},"_dark":{"value":"rgba(57,146,108)"}},"gradient":{"start":{"_light":{"value":"rgba(105,103,103,0.3)"},"_dark":{"value":"rgba(57,146,108,0.3)"}},"stop":{"_light":{"value":"rgba(105,103,103,0)"},"_dark":{"value":"rgba(57,146,108,0)"}}}},"stats":{"bg":{"_light":{"value":"rgba(242,239,239)"},"_dark":{"value":"rgba(255,255,255,0.06)"}}},"topbar":{"bg":{"_light":{"value":"rgba(242,239,239)"},"_dark":{"value":"rgba(255,255,255,0.06)"}}},"navigation":{"text":{"selected":{"_light":{"value":"rgba(25,25,26)"},"_dark":{"value":"rgba(247,250,252)"}}},"bg":{"selected":{"_light":{"value":"rgba(242,239,239)"},"_dark":{"value":"rgba(255,255,255,0.06)"}}}},"tabs":{"text":{"primary":{"_light":{"value":"rgba(25,25,26)"},"_dark":{"value":"rgba(222,217,217)"}}}}}' ],
  ],
  crossChainTxs: [
    [ 'NEXT_PUBLIC_INTERCHAIN_INDEXER_API_HOST', 'http://localhost:3014' ],
    [ 'NEXT_PUBLIC_CROSS_CHAIN_TXS_ENABLED', 'true' ],
  ],
};
