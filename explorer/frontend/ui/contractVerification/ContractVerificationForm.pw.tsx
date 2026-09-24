import React from 'react';

import type { SmartContractVerificationConfig } from 'types/client/contract';

import { ENVS_MAP } from 'playwright/fixtures/mockEnvs';
import { test, expect } from 'playwright/lib';

import ContractVerificationForm from './ContractVerificationForm';

const hooksConfig = {
  router: {
    query: {},
  },
};

const hash = '0x2F99338637F027CFB7494E46B49987457beCC6E3';
const formConfig: SmartContractVerificationConfig = {
  is_rust_verifier_microservice_enabled: true,
  solidity_compiler_versions: [
    'v0.8.17+commit.8df45f5f',
    'v0.8.16+commit.07a7930e',
    'v0.8.15+commit.e14f2714',
    'v0.8.18-nightly.2022.11.23+commit.eb2f874e',
    'v0.8.17-nightly.2022.8.24+commit.22a0c46e',
    'v0.8.16-nightly.2022.7.6+commit.b6f11b33',
  ],
  solidity_evm_versions: [
    'default',
    'london',
    'berlin',
  ],
  verification_options: [
    'flattened-code',
    'standard-input',
    'sourcify',
    'multi-part',
    'vyper-code',
    'vyper-multi-part',
    'vyper-standard-input',
    'solidity-hardhat',
    'solidity-foundry',
  ],
  vyper_compiler_versions: [
    'v0.3.7+commit.6020b8bb',
    'v0.3.1+commit.0463ea4c',
    'v0.3.0+commit.8a23feb',
    'v0.2.16+commit.59e1bdd',
    'v0.2.3+commit.006968f',
    'v0.2.2+commit.337c2ef',
    'v0.1.0-beta.17+commit.0671b7b',
  ],
  vyper_evm_versions: [
    'byzantium',
    'constantinople',
    'petersburg',
    'istanbul',
  ],
  license_types: {
    apache_2_0: 12,
    bsd_2_clause: 8,
    bsd_3_clause: 9,
    bsl_1_1: 14,
    gnu_agpl_v3: 13,
    gnu_gpl_v2: 4,
    gnu_gpl_v3: 5,
    gnu_lgpl_v2_1: 6,
    gnu_lgpl_v3: 7,
    mit: 3,
    mpl_2_0: 10,
    none: 1,
    osl_3_0: 11,
    unlicense: 2,
  },
};

test('flatten source code method +@dark-mode +@mobile', async({ render, page }) => {
  test.slow();
  const component = await render(<ContractVerificationForm config={ formConfig } hash={ hash }/>, { hooksConfig });

  // select license
  await component.locator('button').filter({ hasText: 'Contract license' }).click();
  await page.getByRole('option', { name: 'MIT License' }).click();

  // select method
  await component.locator('button').filter({ hasText: 'Verification method' }).click();
  await page.getByRole('option', { name: 'Solidity (Single file)' }).click();

  await page.getByText(/add contract libraries/i).click();
  await page.locator('button[aria-label="Add item"]').click();

  await expect(component).toHaveScreenshot({ timeout: 10_000 });
});

test('standard input json method', async({ render, page }) => {
  const component = await render(<ContractVerificationForm config={ formConfig } hash={ hash }/>, { hooksConfig });

  // select method
  await component.locator('button').filter({ hasText: 'Verification method' }).click();
  await page.getByRole('option', { name: 'Solidity (Standard JSON input)' }).click();

  await expect(component).toHaveScreenshot();
});

test('sourcify method', async({ render, page }) => {
  await page.route('https://verify.sourcify.dev/widget**', (route) => {
    return route.fulfill({
      status: 200,
      contentType: 'text/html',
      path: './playwright/mocks/page.html',
    });
  });

  const component = await render(
    <ContractVerificationForm config={ formConfig } hash={ hash }/>,
    { hooksConfig },
    { withSocket: true },
  );

  // select method
  await component.locator('button').filter({ hasText: 'Verification method' }).click();
  await page.getByRole('option', { name: 'Sourcify (Solidity or Vyper)' }).click();

  await expect(component).toHaveScreenshot();
});

test('multi-part files method', async({ render, page }) => {
  const component = await render(<ContractVerificationForm config={ formConfig } hash={ hash }/>, { hooksConfig });

  // select method
  await component.locator('button').filter({ hasText: 'Verification method' }).click();
  await page.getByRole('option', { name: 'Solidity (Multi-part files)' }).click();

  await expect(component).toHaveScreenshot();
});

test('vyper contract method', async({ render, page }) => {
  const component = await render(<ContractVerificationForm config={ formConfig } hash={ hash }/>, { hooksConfig });

  // select method
  await component.locator('button').filter({ hasText: 'Verification method' }).click();
  await page.getByRole('option', { name: 'Vyper (Contract)' }).click();

  await expect(component).toHaveScreenshot();
});

test('vyper multi-part method', async({ render, page }) => {
  const component = await render(<ContractVerificationForm config={ formConfig } hash={ hash }/>, { hooksConfig });

  // select method
  await component.locator('button').filter({ hasText: 'Verification method' }).click();
  await page.getByRole('option', { name: 'Vyper (Multi-part files)' }).click();

  await expect(component).toHaveScreenshot();
});

test('vyper vyper-standard-input method', async({ render, page }) => {
  const component = await render(<ContractVerificationForm config={ formConfig } hash={ hash }/>, { hooksConfig });

  // select method
  await component.locator('button').filter({ hasText: 'Verification method' }).click();
  await page.getByRole('option', { name: 'Vyper (Standard JSON input)' }).click();

  await expect(component).toHaveScreenshot();
});

test('solidity-hardhat method', async({ render, page, mockEnvs }) => {
  await mockEnvs([ [ 'NEXT_PUBLIC_API_BASE_PATH', '/blockscout' ] ]);
  const component = await render(<ContractVerificationForm config={ formConfig } hash={ hash }/>, { hooksConfig });

  // select method
  await component.locator('button').filter({ hasText: 'Verification method' }).click();
  await page.getByRole('option', { name: 'Solidity (Hardhat)' }).click();

  await expect(component).toHaveScreenshot();
});

test('solidity-foundry method', async({ render, page, mockEnvs }) => {
  await mockEnvs([ [ 'NEXT_PUBLIC_API_BASE_PATH', '/blockscout' ] ]);
  const component = await render(<ContractVerificationForm config={ formConfig } hash={ hash }/>, { hooksConfig });

  // select method
  await component.locator('button').filter({ hasText: 'Verification method' }).click();
  await page.getByRole('option', { name: 'Solidity (Foundry)' }).click();

  await expect(component).toHaveScreenshot();
});

test('verification of zkSync contract', async({ render, mockEnvs }) => {
  const zkSyncFormConfig: SmartContractVerificationConfig = {
    ...formConfig,
    verification_options: [ 'standard-input' ],
    zk_compiler_versions: [ 'v1.4.1', 'v1.4.0', 'v1.3.23', 'v1.3.22' ],
    zk_optimization_modes: [ '0', '1', '2', '3', 's', 'z' ],
  };

  await mockEnvs(ENVS_MAP.zkSyncRollup);
  const component = await render(<ContractVerificationForm config={ zkSyncFormConfig } hash={ hash }/>, { hooksConfig });

  await expect(component).toHaveScreenshot();
});

test('verification of stylus rust contract', async({ render, page }) => {
  const stylusRustFormConfig: SmartContractVerificationConfig = {
    ...formConfig,
    stylus_compiler_versions: [ 'v0.5.0', 'v0.5.1', 'v0.5.2', 'v0.5.3' ],
    verification_options: formConfig.verification_options.concat([ 'stylus-github-repository' ]),
  };

  const component = await render(<ContractVerificationForm config={ stylusRustFormConfig } hash={ hash }/>, { hooksConfig });

  // select method
  await component.locator('button').filter({ hasText: 'Verification method' }).click();
  await page.getByRole('option', { name: 'Stylus (GitHub repository)' }).click();

  // check validation of github repository field
  const githubRepositoryField = component.getByLabel(/github repository url/i);
  await githubRepositoryField.focus();
  await githubRepositoryField.fill('https://example.com');
  await githubRepositoryField.blur();

  await expect(component.getByText(/invalid github repository url/i)).toBeVisible();

  const DUCK_COMMIT_HASH = '45dd018a19fff2651eb3c23037427a7531af6588';
  const GOOSE_COMMIT_HASH = 'f7e5629';
  await page.route('https://api.github.com/repos/tom2drum/not-duck/commits?per_page=1', (route) => {
    return route.fulfill({ status: 404 });
  }, { times: 1 });
  await page.route('https://api.github.com/repos/tom2drum/duck/commits?per_page=1', (route) => {
    return route.fulfill({ status: 200, json: [ { sha: DUCK_COMMIT_HASH } ] });
  }, { times: 1 });
  await githubRepositoryField.focus();
  await githubRepositoryField.fill('https://github.com/tom2drum/not-duck');
  await githubRepositoryField.blur();

  await expect(component.getByText(/github repository not found/i)).toBeVisible();

  await githubRepositoryField.focus();
  await githubRepositoryField.fill('https://github.com/tom2drum/duck');
  await githubRepositoryField.blur();

  await expect(component.getByText(/github repository not found/i)).toBeHidden();
  await expect(component.getByText(/we have found the latest commit hash/i)).toBeVisible();
  await expect(component.getByText(DUCK_COMMIT_HASH.slice(0, 7))).toBeVisible();

  // check validation of commit hash field
  await page.route(`https://api.github.com/repos/tom2drum/duck/commits/${ GOOSE_COMMIT_HASH }`, (route) => {
    return route.fulfill({ status: 404 });
  }, { times: 1 });
  await page.route(`https://api.github.com/repos/tom2drum/goose/commits/${ GOOSE_COMMIT_HASH }`, (route) => {
    return route.fulfill({ status: 200, json: { sha: GOOSE_COMMIT_HASH } });
  }, { times: 1 });
  await page.route('https://api.github.com/repos/tom2drum/goose/commits?per_page=1', (route) => {
    return route.fulfill({ status: 200, json: [ { sha: GOOSE_COMMIT_HASH } ] });
  }, { times: 1 });
  const commitHashField = component.getByLabel(/commit hash/i);

  await commitHashField.focus();
  await commitHashField.fill(GOOSE_COMMIT_HASH);
  await commitHashField.blur();

  await expect(component.getByText(/commit hash not found in the repository/i)).toBeVisible();

  await githubRepositoryField.focus();
  await githubRepositoryField.fill('https://github.com/tom2drum/goose');
  await githubRepositoryField.blur();

  await expect(component.getByText(/commit hash not found in the repository/i)).toBeHidden();

  await expect(component).toHaveScreenshot();
});
