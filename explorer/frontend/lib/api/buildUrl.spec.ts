import { expect, test, describe } from 'vitest';

import buildUrl from './buildUrl';

test('builds URL for resource without path params', () => {
  const url = buildUrl('general:config_backend_version');
  expect(url).toBe('https://localhost:3003/api/v2/config/backend-version');
});

test('builds URL for resource with path params', () => {
  const url = buildUrl('general:block', { height_or_hash: '42' });
  expect(url).toBe('https://localhost:3003/api/v2/blocks/42');
});

describe('falsy query parameters', () => {
  test('leaves "false" as query parameter', () => {
    const url = buildUrl('general:block', { height_or_hash: '42' }, { includeTx: false });
    expect(url).toBe('https://localhost:3003/api/v2/blocks/42?includeTx=false');
  });

  test('leaves "null" as query parameter', () => {
    const url = buildUrl('general:block', { height_or_hash: '42' }, { includeTx: null });
    expect(url).toBe('https://localhost:3003/api/v2/blocks/42?includeTx=null');
  });

  test('strips out empty string as query parameter', () => {
    const url = buildUrl('general:block', { height_or_hash: '42' }, { includeTx: null, sort: '' });
    expect(url).toBe('https://localhost:3003/api/v2/blocks/42?includeTx=null');
  });

  test('strips out "undefined" as query parameter', () => {
    const url = buildUrl('general:block', { height_or_hash: '42' }, { includeTx: null, sort: undefined });
    expect(url).toBe('https://localhost:3003/api/v2/blocks/42?includeTx=null');
  });
});

test('builds URL with array-like query parameters', () => {
  const url = buildUrl('general:block', { height_or_hash: '42' }, { includeTx: [ '0x11', '0x22' ], sort: 'asc' });
  expect(url).toBe('https://localhost:3003/api/v2/blocks/42?includeTx=0x11%2C0x22&sort=asc');
});

test('builds URL for resource with custom API endpoint', () => {
  const url = buildUrl('contractInfo:token_verified_info', { chainId: '42', hash: '0x11' });
  expect(url).toBe('https://localhost:3005/api/v1/chains/42/token-infos/0x11');
});
