/* eslint-disable no-restricted-properties */
import type { MultichainConfig } from 'types/multichain';

import config from 'configs/app';

function isRunningInDocker() {
  return process.env.HOSTNAME !== undefined;
}

let value: MultichainConfig | undefined = undefined;

async function fetchConfig() {
  if (process.env.NEXT_RUNTIME !== 'edge') {
    throw new Error('NEXT_RUNTIME is not edge');
  }

  // In edge runtime, we need to use absolute URLs
  // When in Docker, use the internal hostname
  const baseUrl = isRunningInDocker() ?
    `http://${ process.env.HOSTNAME }:${ config.app.port || 3000 }` :
    config.app.baseUrl;

  const url = baseUrl + '/assets/essential-dapps/chains.json';
  const response = await fetch(url);
  if (response.ok) {
    const contentType = response.headers.get('content-type');
    if (contentType?.includes('application/json')) {
      const json = await response.json();
      value = json as MultichainConfig;
      return value;
    }
  }
}

export async function load() {
  if (!value) {
    return new Promise<MultichainConfig | undefined>((resolve, reject) => {
      fetchConfig()
        .then((value) => {
          resolve(value);
        }).catch((error) => {
          reject(error);
        });
    });
  }

  return Promise.resolve(value);
}

export function getValue() {
  return value;
}
