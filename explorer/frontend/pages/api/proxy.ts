import { pick, pickBy } from 'es-toolkit';
import type { NextApiRequest, NextApiResponse } from 'next';

import fetchFactory from 'nextjs/utils/fetchProxy';

import appConfig from 'configs/app';
import isNeedProxy from 'lib/api/isNeedProxy';

const handler = async(nextReq: NextApiRequest, nextRes: NextApiResponse) => {

  if (!isNeedProxy()) {
    nextRes.status(404).json({ error: 'Not found' });
    return;
  }

  if (!nextReq.url) {
    nextRes.status(500).json({ error: 'No url provided' });
    return;
  }

  const url = new URL(
    nextReq.url.replace(/^\/node-api\/proxy/, ''),
    nextReq.headers['x-endpoint']?.toString() || appConfig.apis.general?.endpoint,
  );
  const apiRes = await fetchFactory(nextReq)(
    url.toString(),
    pickBy(pick(nextReq, [ 'body', 'method' ]), Boolean),
  );

  // proxy some headers from API
  const HEADERS_TO_PROXY = [
    'x-request-id',
    'content-type',
    'bypass-429-option',
    'x-ratelimit-limit',
    'x-ratelimit-remaining',
    'x-ratelimit-reset',
    'api-v2-temp-token',
  ];

  HEADERS_TO_PROXY.forEach((header) => {
    const value = apiRes.headers.get(header);
    value && nextRes.setHeader(header, value);
  });

  const setCookie = apiRes.headers.raw()['set-cookie'];
  setCookie?.forEach((value) => {
    nextRes.appendHeader('set-cookie', value);
  });

  nextRes.status(apiRes.status).send(apiRes.body);
};

export default handler;

export const config = {
  api: {
    bodyParser: {
      sizeLimit: '100mb',
    },
  },
};
