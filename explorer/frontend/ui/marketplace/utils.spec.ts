import type { NextRouter } from 'next/router';

import { describe, it, expect, beforeEach, vi } from 'vitest';

import { getAppUrl } from './utils';

describe('getAppUrl', () => {
  let router: NextRouter;

  beforeEach(() => {
    router = {
      pathname: '/current/path',
      asPath: '/current/path?someParam=value',
      query: {},
      replace: vi.fn(),
    } as unknown as NextRouter;
  });

  it('should return undefined if url is undefined', () => {
    const result = getAppUrl(undefined, router);
    expect(result).toBeUndefined();
  });

  it('should return the custom url if origins match', () => {
    router.query.url = 'https://example.com/custom-path?query=value';
    const result = getAppUrl('https://example.com/app', router);
    expect(result).toBe('https://example.com/custom-path?query=value');
  });

  it('should remove the query param and return original url if origins do not match', () => {
    router.query.url = 'https://different.com/custom-path?query=value';
    const result = getAppUrl('https://example.com/app', router);
    expect(result).toBe('https://example.com/app?someParam=value');
    expect(router.replace).toHaveBeenCalledWith({ pathname: '/current/path', query: {}, hash: '' }, undefined, { shallow: true });
  });

  it('should construct the new url with custom params and hash', () => {
    router.asPath = '/current/path?path=newPath&newParam=newValue#section';
    const result = getAppUrl('https://example.com/app?existingParam=1', router);
    expect(result).toBe('https://example.com/newPath?existingParam=1&newParam=newValue#section');
  });

  it('should handle url without query and hash', () => {
    router.asPath = '/current/path';
    const result = getAppUrl('https://example.com/app', router);
    expect(result).toBe('https://example.com/app');
  });

  it('should handle error in custom url parsing', () => {
    router.query.url = 'invalid-url';
    const result = getAppUrl('https://example.com/app', router);
    expect(result).toBe('https://example.com/app?someParam=value');
  });

  it('should handle error in target url parsing', () => {
    router.asPath = '/current/path?invalidQuery#section';
    const result = getAppUrl('invalid-url', router);
    expect(result).toBe('invalid-url');
  });
});
