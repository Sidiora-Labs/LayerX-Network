import type { Pool } from 'types/api/pools';

export const base: Pool = {
  pool_id: '0x06da0fd433c1a5d7a4faa01111c044910a184553',
  is_contract: true,
  chain_id: '1',
  base_token_address: '0xdac17f958d2ee523a2206206994597c13d831ec7',
  base_token_symbol: 'USDT',
  base_token_icon_url: 'https://localhost:3000/utia.jpg',
  quote_token_address: '0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2',
  quote_token_symbol: 'WETH',
  quote_token_icon_url: 'https://localhost:3000/secondary_utia.jpg',
  base_token_fully_diluted_valuation_usd: '75486579078',
  base_token_market_cap_usd: '139312819076.195',
  quote_token_fully_diluted_valuation_usd: '486579078',
  quote_token_market_cap_usd: '312819076.195',
  liquidity: '2099941.2238',
  dex: { id: 'sushiswap', name: 'SushiSwap' },
  fee: '0.03',
  coin_gecko_terminal_url: 'https://www.geckoterminal.com/eth/pools/0x06da0fd433c1a5d7a4faa01111c044910a184553',
};

export const noIcons: Pool = {
  ...base,
  base_token_icon_url: null,
  quote_token_icon_url: null,
};
