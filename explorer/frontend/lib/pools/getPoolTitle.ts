import type { Pool } from 'types/api/pools';

export const getPoolTitle = (pool: Pool) => {
  return `${ pool.base_token_symbol } / ${ pool.quote_token_symbol } ${ pool.fee ? `(${ pool.fee }%)` : '' }`;
};
