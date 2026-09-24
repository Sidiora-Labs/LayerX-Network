import type { Abi } from 'abitype';
import { toFunctionSelector } from 'viem';

import type { MethodType, SmartContractMethod, SmartContractMethodRead, SmartContractMethodWrite } from './types';

export const getNativeCoinValue = (value: unknown) => {
  if (typeof value !== 'string') {
    return BigInt(0);
  }

  return BigInt(value);
};

export const isMethod = (method: Abi[number]) =>
  (method.type === 'function' || method.type === 'fallback' || method.type === 'receive');

export const isReadMethod = (method: SmartContractMethod): method is SmartContractMethodRead =>
  (
    method.type === 'function' &&
    (method.constant || method.stateMutability === 'view' || method.stateMutability === 'pure')
  ) || (
    method.type === 'fallback' && method.stateMutability === 'view'
  );

export const isWriteMethod = (method: SmartContractMethod): method is SmartContractMethodWrite =>
  (method.type === 'function' || method.type === 'fallback' || method.type === 'receive') &&
    !isReadMethod(method);

export const enrichWithMethodId = (method: SmartContractMethod): SmartContractMethod => {
  if (method.type !== 'function') {
    return method;
  }

  try {
    return {
      ...method,
      method_id: toFunctionSelector(method),
    };
  } catch (error) {
    return {
      ...method,
      is_invalid: true,
    };
  }
};

export const addInputsToFallback = (method: SmartContractMethod): SmartContractMethod => {
  if (method.type === 'fallback') {
    return {
      ...method,
      inputs: [ { internalType: 'bytes', name: 'input', type: 'bytes' } ],
      outputs: [ { internalType: 'bytes', name: 'output', type: 'bytes' } ],
    };
  }

  return method;
};

const getNameForSorting = (method: SmartContractMethod) => {
  if ('name' in method) {
    return method.name;
  }

  return method.type === 'fallback' ? 'fallback' : 'receive';
};

export const formatAbi = (abi: Abi) => {

  const methods = abi.filter(isMethod) as Array<SmartContractMethod>;

  return methods
    .map(enrichWithMethodId)
    .map(addInputsToFallback)
    .sort((a, b) => {
      const aName = getNameForSorting(a);
      const bName = getNameForSorting(b);
      return aName.localeCompare(bName);
    });
};

export const TYPE_FILTER_OPTIONS: Array<{ value: MethodType; title: string }> = [
  { value: 'all', title: 'All' },
  { value: 'read', title: 'Read' },
  { value: 'write', title: 'Write' },
];
