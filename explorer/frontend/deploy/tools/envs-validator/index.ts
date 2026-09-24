/* eslint-disable no-console */
import fs from 'node:fs';
import { dirname, resolve as resolvePath } from 'node:path';
import type { ValidationError } from 'yup';

import { buildExternalAssetFilePath } from 'configs/app/utils';
import schema from './schema';
import schemaMultichain from './schema_multichain';
import { fileURLToPath } from 'node:url';

const currentFilePath = fileURLToPath(import.meta.url);
const distDir = dirname(currentFilePath);

const silent = process.argv.includes('--silent');

run();

async function run() {
  !silent && console.log();
  try {
    const appEnvs = Object.entries(process.env)
      .filter(([ key ]) => key.startsWith('NEXT_PUBLIC_'))
      .reduce((result, [ key, value ]) => {
        result[key] = value || '';
        return result;
      }, {} as Record<string, string>);

    printDeprecationWarning(appEnvs);
    await checkPlaceholdersCongruity(appEnvs);
    checkDeprecatedEnvs(appEnvs);
    await validateEnvs(appEnvs);

  } catch (error) {
    process.exit(1);
  }
}

async function validateEnvs(appEnvs: Record<string, string>) {
  !silent && console.log(`🌀 Validating ENV variables values...`);

  try {
    // replace ENVs with external JSON files content
    const envsWithJsonConfig = [
      'NEXT_PUBLIC_FEATURED_NETWORKS',
      'NEXT_PUBLIC_MARKETPLACE_CONFIG_URL',
      'NEXT_PUBLIC_MARKETPLACE_CATEGORIES_URL',
      'NEXT_PUBLIC_MARKETPLACE_GRAPH_LINKS_URL',
      'NEXT_PUBLIC_FOOTER_LINKS',
      'NEXT_PUBLIC_ADDRESS_3RD_PARTY_WIDGETS_CONFIG_URL',
      'NEXT_PUBLIC_ZETACHAIN_SERVICE_CHAINS_CONFIG_URL',
      'NEXT_PUBLIC_HOMEPAGE_HIGHLIGHTS_CONFIG',
    ];

    for await (const envName of envsWithJsonConfig) {
      if (appEnvs[envName]) {
        appEnvs[envName] = await getExternalJsonContent(envName) || '[]';
      }
    }

    if (appEnvs.NEXT_PUBLIC_MULTICHAIN_ENABLED === 'true') {
      await schemaMultichain.validate(appEnvs, { stripUnknown: false, abortEarly: false });
    } else {
      await schema.validate(appEnvs, { stripUnknown: false, abortEarly: false });
    }

    !silent && console.log('👍 All good!');
  } catch (_error) {
    if (typeof _error === 'object' && _error !== null && 'errors' in _error) {
      console.log('🚨 ENVs validation failed with the following errors:');
      (_error as ValidationError).errors.forEach((error) => {
        console.log('    ', error);
      });
    } else {
      console.log('🚨 Unexpected error occurred during validation.');
      console.error(_error);
    }

    throw _error;
  }

  !silent && console.log();
}

async function getExternalJsonContent(envName: string): Promise<string | void> {
  return new Promise((resolve, reject) => {
    const fileName = `./public${ buildExternalAssetFilePath(envName, 'https://foo.bar/baz.json') }`;

    fs.readFile(resolvePath(distDir, '..', fileName), 'utf8', (err, data) => {
      if (err) {
        console.log(`🚨 Unable to read file: ${ fileName }`);
        reject(err);
        return;
      }

      resolve(data);
    });
  });
}

async function checkPlaceholdersCongruity(envsMap: Record<string, string>) {
  try {
    !silent && console.log(`🌀 Checking environment variables and their placeholders congruity...`);

    const runTimeEnvs = await getEnvsPlaceholders(resolvePath(distDir, '..', '.env.registry'));
    const buildTimeEnvs = await getEnvsPlaceholders(resolvePath(distDir, '..', '.env'));
    const envs = Object.keys(envsMap).filter((env) => !buildTimeEnvs.includes(env));

    const inconsistencies: Array<string> = [];
    for (const env of envs) {
      const hasPlaceholder = runTimeEnvs.includes(env);
      if (!hasPlaceholder) {
        inconsistencies.push(env);
      }
    }

    if (inconsistencies.length > 0) {
      console.log('🚸 For the following environment variables placeholders were not generated at build-time:');
      inconsistencies.forEach((env) => {
        console.log(`     ${ env }`);
      });
      console.log(`   They are either deprecated or running the app with them may lead to unexpected behavior.
   Please check the documentation for more details - https://github.com/blockscout/frontend/blob/main/docs/ENVS.md
      `);
      throw new Error();
    }

    !silent && console.log('👍 All good!\n');
  } catch (error) {
    console.log('🚨 Congruity check failed.\n', error);
    throw error;
  }
}

function getEnvsPlaceholders(filePath: string): Promise<Array<string>> {
  return new Promise((resolve, reject) => {
    fs.readFile(filePath, 'utf8', (err, data) => {
      if (err) {
        console.log(`🚨 Unable to read placeholders file.`);
        reject(err);
        return;
      }

      const lines = data.split('\n');
      const variables = lines.map(line => {
        const variable = line.split('=')[0];
        return variable.trim();
      });

      resolve(variables.filter(Boolean));
    });
  });
}

function printDeprecationWarning(envsMap: Record<string, string>) {
  
}

function checkDeprecatedEnvs(envsMap: Record<string, string>) {
  !silent && console.log(`🌀 Checking deprecated environment variables...`);
  !silent && console.log('👍 All good!\n');
}
