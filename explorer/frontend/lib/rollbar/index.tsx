import { Provider as DefaultProvider, useRollbar as useRollbarDefault } from '@rollbar/react';
import type React from 'react';
import type { Configuration } from 'rollbar';

import config from 'configs/app';
import { ABSENT_PARAM_ERROR_MESSAGE } from 'lib/errors/throwOnAbsentParamError';
import { RESOURCE_LOAD_ERROR_MESSAGE } from 'lib/errors/throwOnResourceLoadError';

import { isBot, isHeadlessBrowser, isNextJsChunkError, getRequestInfo, getExceptionClass, getExceptionOriginFileName } from './utils';

const feature = config.features.rollbar;

const FallbackProvider = ({ children }: { children: React.ReactNode }) => children;

const useRollbarFallback = (): undefined => {};

export const Provider = feature.isEnabled ? DefaultProvider : FallbackProvider;
export const useRollbar = feature.isEnabled ? useRollbarDefault : useRollbarFallback;

export const clientConfig: Configuration | undefined = feature.isEnabled ? {
  accessToken: feature.clientToken,
  environment: feature.environment,
  payload: {
    code_version: feature.codeVersion,
    app_instance: feature.instance,
  },
  checkIgnore(isUncaught, args, item) {
    if (isBot(window.navigator.userAgent)) {
      return true;
    }

    if (isHeadlessBrowser(window.navigator.userAgent)) {
      return true;
    }

    if (isNextJsChunkError(getRequestInfo(item)?.url)) {
      return true;
    }

    const exceptionClass = getExceptionClass(item);
    const IGNORED_EXCEPTION_CLASSES = [
      // these are React errors - "NotFoundError: Failed to execute 'removeChild' on 'Node': The node to be removed is not a child of this node."
      // they could be caused by browser extensions
      // one of the examples - https://github.com/facebook/react/issues/11538
      // we can ignore them for now
      'NotFoundError',

      'AbortError',
    ];

    if (exceptionClass && IGNORED_EXCEPTION_CLASSES.includes(exceptionClass)) {
      return true;
    }

    const originFileName = getExceptionOriginFileName(item);
    const IGNORED_ORIGIN_FILE_NAMES_CHUNKS = [
      '/node_modules/@walletconnect',
      '/node_modules/@reown',
    ];

    if (originFileName && IGNORED_ORIGIN_FILE_NAMES_CHUNKS.some((chunk) => originFileName.includes(chunk))) {
      return true;
    }

    return false;
  },
  hostSafeList: [ config.app.host ].filter(Boolean),
  ignoredMessages: [
    // these are errors that we throw on when make a call to the API
    RESOURCE_LOAD_ERROR_MESSAGE,
    ABSENT_PARAM_ERROR_MESSAGE,

    // Filter out network-related errors that are usually not actionable
    'Network Error',
    'Failed to fetch',

    // Filter out CORS errors from third-party extensions
    'cross-origin',

    // Filter out client-side navigation cancellations
    'cancelled navigation',
  ],
  maxItems: 10, // Max items per page load
} : undefined;
