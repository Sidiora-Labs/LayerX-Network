import type { AbiFunction, AbiFallback as AbiFallbackViem, AbiReceive } from 'abitype';
import type { AbiParameter, AbiStateMutability } from 'viem';

export type ContractAbiItemInput = AbiFunction['inputs'][number] & { fieldType?: 'native_coin' };

export type MethodType = 'read' | 'write' | 'all';
export type MethodCallStrategy = 'read' | 'write' | 'simulate' | 'copy_calldata';
export type ResultViewMode = 'preview' | 'result';

// we manually add inputs and outputs to the fallback method because viem doesn't support it
// but as we discussed with @k1rill-fedoseev, it's a good idea to have them for fallback method of any contract
// also, according to @k1rill-fedoseev, fallback method can act as a read method when it has 'view' state mutability
// but viem doesn't aware of this and thinks that fallback method state mutability can only be 'payable' or 'nonpayable'
// so we have to redefine the stateMutability as well to include "view" option
// see "addInputsToFallback" and "isReadMethod" functions in utils.ts
export interface AbiFallback extends Pick<AbiFallbackViem, 'type' | 'payable'> {
  inputs: Array<AbiParameter>;
  outputs: Array<AbiParameter>;
  stateMutability: Exclude<AbiStateMutability, 'pure'>;
}

export type SmartContractMethodCustomFields = { method_id: string } | { is_invalid: boolean };
export type SmartContractMethodRead = AbiFunction & SmartContractMethodCustomFields;
export type SmartContractMethodWrite = AbiFunction & SmartContractMethodCustomFields | AbiFallback | AbiReceive;
export type SmartContractMethod = SmartContractMethodRead | SmartContractMethodWrite;

export interface FormSubmitResultPublicClient {
  source: 'public_client';
  data: unknown | Error;
  estimatedGas?: bigint;
}
export interface FormSubmitResultWalletClient {
  source: 'wallet_client';
  data: { hash: `0x${ string }` } | Error;
}
export type FormSubmitResult = FormSubmitResultPublicClient | FormSubmitResultWalletClient;

export type FormSubmitHandler = (item: SmartContractMethod, args: Array<unknown>, submitType: MethodCallStrategy | undefined) => Promise<FormSubmitResult>;
