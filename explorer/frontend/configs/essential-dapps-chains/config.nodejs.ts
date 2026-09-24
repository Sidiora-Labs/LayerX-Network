import type { MultichainConfig } from 'types/multichain';

let value: MultichainConfig | undefined = undefined;

function readFileConfig() {
  // eslint-disable-next-line no-restricted-properties
  if (process.env.NEXT_RUNTIME !== 'nodejs') {
    throw new Error('NEXT_RUNTIME is not nodejs');
  }

  try {
    const path = require('path');
    const { readFileSync } = require('fs');
    const publicFolder = path.resolve('public');
    const configPath = path.resolve(publicFolder, 'assets/essential-dapps/chains.json');

    const config = readFileSync(configPath, 'utf8');

    value = JSON.parse(config) as MultichainConfig;
    return value;
  } catch (error) {
    return;
  }
}

export async function load() {
  if (!value) {
    return new Promise<MultichainConfig | undefined>((resolve) => {
      const value = readFileConfig();
      resolve(value);
    });
  }

  return Promise.resolve(value);
}

export function getValue() {
  if (!value) {
    return readFileConfig();
  }

  return value;
}
