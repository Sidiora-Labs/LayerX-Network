import type { AddressParam } from 'types/api/addressParams';
import type { TokenInfo } from 'types/api/token';

export interface TxInterpretationResponse {
  data: {
    summaries: Array<TxInterpretationSummary>;
  };
}

export type TxInterpretationSummary = {
  summary_template: string;
  summary_template_variables: Record<string, TxInterpretationVariable>;
};

export type TxInterpretationVariable =
  TxInterpretationVariableString |
  TxInterpretationVariableCurrency |
  TxInterpretationVariableTimestamp |
  TxInterpretationVariableExternalLink |
  TxInterpretationVariableToken |
  TxInterpretationVariableAddress |
  TxInterpretationVariableDomain |
  TxInterpretationVariableMethod |
  TxInterpretationVariableDex |
  TxInterpretationVariableLink;

export type TxInterpretationVariableType =
  'string' |
  'currency' |
  'timestamp' |
  'external_link' |
  'token' |
  'address' |
  'domain' |
  'method' |
  'dexTag' |
  'link';

export type TxInterpretationVariableString = {
  type: 'string';
  value: string;
};

export type TxInterpretationVariableCurrency = {
  type: 'currency';
  value: string;
};

export type TxInterpretationVariableTimestamp = {
  type: 'timestamp';
  value: string;
};

export type TxInterpretationVariableExternalLink = {
  type: 'external_link';
  value: {
    name: string;
    link: string;
  };
};

export type TxInterpretationVariableToken = {
  type: 'token';
  value: TokenInfo;
};

export type TxInterpretationVariableAddress = {
  type: 'address';
  value: AddressParam;
};

export type TxInterpretationVariableDomain = {
  type: 'domain';
  value: string;
};

export type TxInterpretationVariableMethod = {
  type: 'method';
  value: string;
};

export type TxInterpretationVariableDex = {
  type: 'dexTag';
  value: {
    name: string;
    icon: string;
    url: string;
    app_id?: string;
    app_icon?: string;
  };
};

export type TxInterpretationVariableLink = {
  type: 'link';
  value: {
    name: string;
    url: string;
  };
};
