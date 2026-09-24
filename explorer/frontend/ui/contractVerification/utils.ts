import type { FieldPath, ErrorOption } from 'react-hook-form';

import type {
  ContractLibrary,
  FormFields,
  FormFieldsFlattenSourceCode,
  FormFieldsMultiPartFile,
  FormFieldsStandardInput,
  FormFieldsStandardInputZk,
  FormFieldsStylusGitHubRepo,
  FormFieldsVyperContract,
  FormFieldsVyperMultiPartFile,
  FormFieldsVyperStandardInput,
} from './types';
import type {
  SmartContractVerificationError,
  SmartContractLicenseType,
} from 'types/api/contract';
import type { SmartContractVerificationConfig, SmartContractVerificationMethod } from 'types/client/contract';

import type { Params as FetchParams } from 'lib/hooks/useFetch';
import { stripLeadingSlash } from 'toolkit/utils/url';

export const SUPPORTED_VERIFICATION_METHODS: Array<SmartContractVerificationMethod> = [
  'flattened-code',
  'standard-input',
  'sourcify',
  'multi-part',
  'solidity-hardhat',
  'solidity-foundry',
  'vyper-code',
  'vyper-multi-part',
  'vyper-standard-input',
  'stylus-github-repository',
];

export const METHOD_LABELS: Record<SmartContractVerificationMethod, string> = {
  'flattened-code': 'Solidity (Single file)',
  'standard-input': 'Solidity (Standard JSON input)',
  sourcify: 'Sourcify (Solidity or Vyper)',
  'multi-part': 'Solidity (Multi-part files)',
  'vyper-code': 'Vyper (Contract)',
  'vyper-multi-part': 'Vyper (Multi-part files)',
  'vyper-standard-input': 'Vyper (Standard JSON input)',
  'solidity-hardhat': 'Solidity (Hardhat)',
  'solidity-foundry': 'Solidity (Foundry)',
  'stylus-github-repository': 'Stylus (GitHub repository)',
};

export const DEFAULT_VALUES: Record<SmartContractVerificationMethod, FormFields> = {
  'flattened-code': {
    address: '',
    method: [ 'flattened-code' ],
    is_yul: false,
    name: '',
    compiler: [],
    evm_version: [],
    is_optimization_enabled: true,
    optimization_runs: '200',
    code: '',
    autodetect_constructor_args: true,
    constructor_args: '',
    libraries: [],
    license_type: [],
  },
  'standard-input': {
    address: '',
    method: [ 'standard-input' ],
    name: '',
    compiler: [],
    sources: [],
    autodetect_constructor_args: true,
    constructor_args: '',
    license_type: [],
  },
  sourcify: {
    address: '',
    method: [ 'sourcify' ],
    sources: [],
    license_type: [],
  },
  'multi-part': {
    address: '',
    method: [ 'multi-part' ],
    compiler: [],
    evm_version: [],
    is_optimization_enabled: true,
    optimization_runs: '200',
    sources: [],
    libraries: [],
    license_type: [],
  },
  'vyper-code': {
    address: '',
    method: [ 'vyper-code' ],
    name: '',
    compiler: [],
    evm_version: [],
    code: '',
    constructor_args: '',
    license_type: [],
  },
  'vyper-multi-part': {
    address: '',
    method: [ 'vyper-multi-part' ],
    compiler: [],
    evm_version: [],
    sources: [],
    license_type: [],
  },
  'vyper-standard-input': {
    address: '',
    method: [ 'vyper-standard-input' ],
    compiler: [],
    sources: [],
    license_type: [],
  },
  'solidity-hardhat': {
    address: '',
    method: [ 'solidity-hardhat' ],
    compiler: [],
    sources: [],
    license_type: [],
  },
  'solidity-foundry': {
    address: '',
    method: [ 'solidity-foundry' ],
    compiler: [],
    sources: [],
    license_type: [],
  },
  'stylus-github-repository': {
    address: '',
    method: [ 'stylus-github-repository' ],
    compiler: [],
    repository_url: '',
    commit_hash: '',
    path_prefix: '',
    license_type: [],
  },
};

export function getDefaultValues(
  methodParam: SmartContractVerificationMethod | undefined,
  config: SmartContractVerificationConfig,
  hash: string | undefined,
  licenseType: FormFields['license_type'],
) {
  const singleMethod = config.verification_options.length === 1 ? config.verification_options[0] : undefined;
  const method = singleMethod || methodParam;

  if (!method) {
    return { address: hash || '' };
  }

  const defaultValues: FormFields = { ...DEFAULT_VALUES[method], address: hash || '', license_type: licenseType };

  if ('evm_version' in defaultValues) {
    if (method === 'flattened-code' || method === 'multi-part') {
      defaultValues.evm_version = config.solidity_evm_versions.find((value) => value === 'default') ? [ 'default' ] : [];
    }

    if (method === 'vyper-multi-part') {
      defaultValues.evm_version = config.vyper_evm_versions.find((value) => value === 'default') ? [ 'default' ] : [];
    }
  }

  if (config.is_rust_verifier_microservice_enabled) {
    if (method === 'flattened-code' || method === 'standard-input') {
      'name' in defaultValues && (defaultValues.name = undefined);
      'autodetect_constructor_args' in defaultValues && (defaultValues.autodetect_constructor_args = false);
    }
  }

  if (singleMethod) {
    defaultValues.method = config.verification_options;
  }

  return defaultValues;
}

export function isValidVerificationMethod(method?: string): method is SmartContractVerificationMethod {
  return method && SUPPORTED_VERIFICATION_METHODS.includes(method) ? true : false;
}

export function sortVerificationMethods(methodA: SmartContractVerificationMethod, methodB: SmartContractVerificationMethod) {
  const indexA = SUPPORTED_VERIFICATION_METHODS.indexOf(methodA);
  const indexB = SUPPORTED_VERIFICATION_METHODS.indexOf(methodB);

  if (indexA > indexB) {
    return 1;
  }

  if (indexA < indexB) {
    return -1;
  }

  return 0;
}

export function prepareRequestBody(data: FormFields): FetchParams['body'] {
  const defaultLicenseType: SmartContractLicenseType = 'none';

  switch (data.method[0]) {
    case 'flattened-code': {
      const _data = data as FormFieldsFlattenSourceCode;
      return {
        compiler_version: _data.compiler?.[0],
        source_code: _data.code,
        is_optimization_enabled: _data.is_optimization_enabled,
        is_yul_contract: _data.is_yul,
        optimization_runs: _data.optimization_runs,
        contract_name: _data.name || undefined,
        libraries: reduceLibrariesArray(_data.libraries),
        evm_version: _data.evm_version?.[0],
        autodetect_constructor_args: _data.autodetect_constructor_args,
        constructor_args: _data.constructor_args,
        license_type: _data.license_type?.[0] ?? defaultLicenseType,
      };
    }

    case 'standard-input': {
      const _data = data as (FormFieldsStandardInput | FormFieldsStandardInputZk);

      const body = new FormData();
      _data.compiler && body.set('compiler_version', _data.compiler?.[0]);
      body.set('license_type', _data.license_type?.[0] ?? defaultLicenseType);
      body.set('contract_name', _data.name);
      body.set('autodetect_constructor_args', String(Boolean(_data.autodetect_constructor_args)));
      body.set('constructor_args', _data.constructor_args);
      addFilesToFormData(body, _data.sources, 'files');

      // zkSync fields
      'zk_compiler' in _data && _data.zk_compiler && body.set('zk_compiler_version', _data.zk_compiler?.[0]);

      return body;
    }

    case 'multi-part': {
      const _data = data as FormFieldsMultiPartFile;

      const body = new FormData();
      _data.compiler && body.set('compiler_version', _data.compiler?.[0]);
      _data.evm_version && body.set('evm_version', _data.evm_version?.[0]);
      body.set('license_type', _data.license_type?.[0] ?? defaultLicenseType);
      body.set('is_optimization_enabled', String(Boolean(_data.is_optimization_enabled)));
      _data.is_optimization_enabled && body.set('optimization_runs', _data.optimization_runs);

      const libraries = reduceLibrariesArray(_data.libraries);
      libraries && body.set('libraries', JSON.stringify(libraries));
      addFilesToFormData(body, _data.sources, 'files');

      return body;
    }

    case 'vyper-code': {
      const _data = data as FormFieldsVyperContract;

      return {
        compiler_version: _data.compiler?.[0],
        evm_version: _data.evm_version?.[0],
        source_code: _data.code,
        contract_name: _data.name,
        constructor_args: _data.constructor_args,
        license_type: _data.license_type?.[0] ?? defaultLicenseType,
      };
    }

    case 'vyper-multi-part': {
      const _data = data as FormFieldsVyperMultiPartFile;

      const body = new FormData();
      _data.compiler && body.set('compiler_version', _data.compiler?.[0]);
      _data.evm_version && body.set('evm_version', _data.evm_version?.[0]);
      body.set('license_type', _data.license_type?.[0] ?? defaultLicenseType);
      addFilesToFormData(body, _data.sources, 'files');
      addFilesToFormData(body, _data.interfaces, 'interfaces');

      return body;
    }

    case 'vyper-standard-input': {
      const _data = data as FormFieldsVyperStandardInput;

      const body = new FormData();
      _data.compiler && body.set('compiler_version', _data.compiler?.[0]);
      body.set('license_type', _data.license_type?.[0] ?? defaultLicenseType);
      addFilesToFormData(body, _data.sources, 'files');

      return body;
    }

    case 'stylus-github-repository': {
      const _data = data as FormFieldsStylusGitHubRepo;

      return {
        cargo_stylus_version: _data.compiler?.[0],
        repository_url: _data.repository_url,
        commit: _data.commit_hash,
        path_prefix: _data.path_prefix,
        license_type: _data.license_type?.[0] ?? defaultLicenseType,
      };
    }

    default: {
      return {};
    }
  }
}

function reduceLibrariesArray(libraries: Array<ContractLibrary> | undefined) {
  if (!libraries || libraries.length === 0) {
    return;
  }

  if (libraries.every((item) => item.name === '' && item.address === '')) {
    return;
  }

  return libraries.reduce<Record<string, string>>((result, item) => {
    result[item.name] = item.address;
    return result;
  }, {});
}

function addFilesToFormData(body: FormData, files: Array<File> | undefined, fieldName: 'files' | 'interfaces') {
  if (!files) {
    return;
  }

  for (let index = 0; index < files.length; index++) {
    const file = files[index];
    body.set(`${ fieldName }[${ index }]`, file, file.name);
  }
}

const API_ERROR_TO_FORM_FIELD: Record<keyof SmartContractVerificationError, FieldPath<FormFields>> = {
  contract_source_code: 'code',
  files: 'sources',
  interfaces: 'interfaces',
  compiler_version: 'compiler',
  constructor_arguments: 'constructor_args',
  name: 'name',
};

export function formatSocketErrors(errors: SmartContractVerificationError): Array<[FieldPath<FormFields>, ErrorOption] | undefined> {
  return Object.entries(errors).map(([ key, value ]) => {
    const _key = key as keyof SmartContractVerificationError;
    if (!API_ERROR_TO_FORM_FIELD[_key]) {
      return;
    }

    return [ API_ERROR_TO_FORM_FIELD[_key], { message: value.join(',') } ];
  });
}

export function getGitHubOwnerAndRepo(repositoryUrl: string) {
  try {
    const urlObj = new URL(repositoryUrl);
    if (urlObj.hostname !== 'github.com') {
      throw new Error();
    }
    const [ owner, repo, ...rest ] = stripLeadingSlash(urlObj.pathname).split('/');
    return { owner, repo, rest, url: urlObj };
  } catch (error) {
    return;
  }
}
