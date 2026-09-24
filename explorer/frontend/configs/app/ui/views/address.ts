import type { SmartContractVerificationMethodExtra } from 'types/client/contract';
import { SMART_CONTRACT_EXTRA_VERIFICATION_METHODS } from 'types/client/contract';
import type { AddressFormat, AddressViewId, IdenticonType } from 'types/views/address';
import { ADDRESS_FORMATS, ADDRESS_VIEWS_IDS, IDENTICON_TYPES } from 'types/views/address';

import { getEnvValue, parseEnvJson } from 'configs/app/utils';

const identiconType: IdenticonType = (() => {
  const value = getEnvValue('NEXT_PUBLIC_VIEWS_ADDRESS_IDENTICON_TYPE');

  return IDENTICON_TYPES.find((type) => value === type) || 'blockie';
})();

const formats: Array<AddressFormat> = (() => {
  const value = (parseEnvJson<Array<AddressFormat>>(getEnvValue('NEXT_PUBLIC_VIEWS_ADDRESS_FORMAT')) || [])
    .filter((format) => ADDRESS_FORMATS.includes(format));

  if (value.length === 0) {
    return [ 'base16' ];
  }

  return value;
})();

const bech32Prefix = (() => {
  const value = getEnvValue('NEXT_PUBLIC_VIEWS_ADDRESS_BECH_32_PREFIX');

  if (!value || !formats.includes('bech32')) {
    return undefined;
  }

  // these are the limits of the bech32 prefix - https://github.com/bitcoin/bips/blob/master/bip-0173.mediawiki#bech32
  return value.length >= 1 && value.length <= 83 ? value : undefined;
})();

const hiddenViews = (() => {
  const parsedValue = parseEnvJson<Array<AddressViewId>>(getEnvValue('NEXT_PUBLIC_VIEWS_ADDRESS_HIDDEN_VIEWS')) || [];

  if (!Array.isArray(parsedValue)) {
    return undefined;
  }

  const result = ADDRESS_VIEWS_IDS.reduce((result, item) => {
    result[item] = parsedValue.includes(item);
    return result;
  }, {} as Record<AddressViewId, boolean>);

  return result;
})();

const extraVerificationMethods: Array<SmartContractVerificationMethodExtra> = (() => {
  const envValue = getEnvValue('NEXT_PUBLIC_VIEWS_CONTRACT_EXTRA_VERIFICATION_METHODS');
  if (envValue === 'none') {
    return [];
  }

  if (!envValue) {
    return SMART_CONTRACT_EXTRA_VERIFICATION_METHODS;
  }

  const parsedMethods = parseEnvJson<Array<SmartContractVerificationMethodExtra>>(envValue) || [];

  return parsedMethods.filter((method) => SMART_CONTRACT_EXTRA_VERIFICATION_METHODS.includes(method));
})();

const config = Object.freeze({
  identiconType,
  hashFormat: {
    availableFormats: formats,
    bech32Prefix,
  },
  hiddenViews,
  nativeTokenAddress: getEnvValue('NEXT_PUBLIC_VIEWS_ADDRESS_NATIVE_TOKEN_ADDRESS'),
  solidityscanEnabled: getEnvValue('NEXT_PUBLIC_VIEWS_CONTRACT_SOLIDITYSCAN_ENABLED') === 'true',
  extraVerificationMethods,
  decodedBytecodeEnabled: getEnvValue('NEXT_PUBLIC_VIEWS_CONTRACT_DECODED_BYTECODE_ENABLED') === 'true',
});

export default config;
