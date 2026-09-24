import type { VerifiedAddress } from 'types/api/account';

export interface AddressVerificationFormFirstStepFields {
  address: string;
}

export interface AddressVerificationFormSecondStepFields {
  signature: string;
  message: string;
}

export interface RootFields {
  root: string;
}

export interface AddressCheckStatusSuccess {
  contractCreator?: string;
  contractOwner?: string;
  signingMessage: string;
}

export type AddressCheckResponseSuccess = {
  status: 'SUCCESS';
  result: AddressCheckStatusSuccess;
} |
{ status: 'IS_OWNER_ERROR' } |
{ status: 'OWNERSHIP_VERIFIED_ERROR' } |
{ status: 'SOURCE_CODE_NOT_VERIFIED_ERROR' } |
{ status: 'INVALID_ADDRESS_ERROR' };

export interface AddressVerificationResponseError {
  code: number;
  message: string;
}

export type AddressValidationResponseSuccess = {
  status: 'SUCCESS';
  result: {
    verifiedAddress: VerifiedAddress;
  };
} |
{
  status: 'INVALID_SIGNER_ERROR';
  invalidSigner: {
    signer: string;
  };
} |
{ status: 'VALIDITY_EXPIRED_ERROR' } |
{ status: 'INVALID_SIGNATURE_ERROR' } |
{ status: 'UNKNOWN_STATUS' };
