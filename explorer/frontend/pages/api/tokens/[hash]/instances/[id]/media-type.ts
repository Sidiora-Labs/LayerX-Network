import type { NextApiRequest, NextApiResponse } from 'next';
import nodeFetch from 'node-fetch';

import fetchApi from 'nextjs/utils/fetchApi';
import { httpLogger } from 'nextjs/utils/logger';

import metrics from 'lib/monitoring/metrics';
import getQueryParamString from 'lib/router/getQueryParamString';
import { SECOND } from 'toolkit/utils/consts';

export default async function tokenInstanceMediaTypeHandler(req: NextApiRequest, res: NextApiResponse) {
  const controller = new AbortController();
  const timeout = setTimeout(() => {
    controller.abort('Request to media asset timed out');
  }, 10 * SECOND);

  try {
    const field = getQueryParamString(req.query.field) as 'animation_url' | 'image_url';
    if (![ 'animation_url', 'image_url' ].includes(field)) {
      throw new Error('Invalid field parameter');
    }

    const apiData = await fetchApi({
      resource: 'general:token_instance',
      pathParams: { hash: getQueryParamString(req.query.hash), id: getQueryParamString(req.query.id) },
      timeout: SECOND,
    });
    if (!apiData) {
      throw new Error('Failed to fetch token instance');
    }

    const mediaUrl = apiData[field];
    if (!mediaUrl) {
      throw new Error('No media URL found');
    }

    const mediaRequestEndTime = metrics?.apiRequestDuration.startTimer();
    const mediaResponse = await nodeFetch(mediaUrl, { method: 'HEAD', signal: controller.signal });
    const mediaRequestDuration = mediaRequestEndTime?.({ route: '/media-type', code: mediaResponse.status });

    if (mediaResponse.status === 200) {
      httpLogger.logger.info({ message: 'API fetch', url: mediaUrl, code: mediaResponse.status, duration: mediaRequestDuration });
    } else {
      httpLogger.logger.error({ message: 'API fetch', url: mediaUrl, code: mediaResponse.status, duration: mediaRequestDuration });
      throw new Error();
    }

    const contentType = mediaResponse.headers.get('content-type');
    const mediaType = (() => {
      if (contentType?.startsWith('video')) {
        return 'video';
      }

      if (contentType?.startsWith('image')) {
        return 'image';
      }

      if (contentType?.startsWith('text/html')) {
        return 'html';
      }
    })();

    res.status(200).json({ type: mediaType });
  } catch (error) {
    res.status(200).json({ type: undefined });
  } finally {
    clearTimeout(timeout);
  }
}
