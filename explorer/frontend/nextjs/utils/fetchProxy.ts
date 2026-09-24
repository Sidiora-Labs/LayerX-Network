import { pick } from 'es-toolkit';
import type { IncomingMessage } from 'http';
import type { NextApiRequest } from 'next';
import type { NextApiRequestCookies } from 'next/dist/server/api-utils';
import type { RequestInit, Response } from 'node-fetch';
import nodeFetch from 'node-fetch';

import { httpLogger } from 'nextjs/utils/logger';

export default function fetchFactory(
  _req: NextApiRequest | (IncomingMessage & { cookies: NextApiRequestCookies }),
) {
  // first arg can be only a string
  // FIXME migrate to RequestInfo later if needed
  return function fetch(url: string, init?: RequestInit): Promise<Response> {
    const cookie = Object.entries(_req.cookies)
      .map(([ key, value ]) => `${ key }=${ value }`)
      .join('; ');

    const headers = {
      accept: _req.headers['accept'] || 'application/json',
      'content-type': _req.headers['content-type'] || 'application/json',
      cookie,
      ...pick(_req.headers, [
        'x-csrf-token',
        'recaptcha-v2-response',
        'user-agent',
        'Authorization', // the old value, just in case
        'authorization', // Node.js automatically lowercases headers
        'show-scam-tokens',
        'api-v2-temp-token',
        // feature flags
        'updated-gas-oracle',
      ]) as Record<string, string | undefined>,
    };

    httpLogger.logger.info({
      message: 'API fetch via Next.js proxy',
      url,
      // headers,
      // init,
    });

    const body = (() => {
      const _body = init?.body;
      if (!_body) {
        return;
      }

      if (typeof _body === 'string') {
        return _body;
      }

      return JSON.stringify(_body);
    })();

    return nodeFetch(url, {
      ...init,
      headers,
      body,
    });
  };
}
