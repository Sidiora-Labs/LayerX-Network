import type { NovesResponseData } from 'types/api/noves';
import type { TxInterpretationSummary } from 'types/api/txInterpretation';

import { createAddressValues } from './getAddressValues';
import type { NovesTokenInfo, TokensData } from './getTokensData';
import { getTokensData } from './getTokensData';

export interface SummaryAddress {
  hash: string;
  name?: string | null;
  is_contract?: boolean;
}

export interface SummaryValues {
  match: string;
  value: NovesTokenInfo | SummaryAddress;
  type: 'token' | 'address';
}

interface NovesSummary {
  summary_template: string;
  summary_template_variables: { [x: string]: unknown };
}

export const createNovesSummaryObject = (translateData: NovesResponseData) => {

  // Remove final dot and add space at the start to avoid matching issues
  const description = translateData.classificationData.description;
  const removedFinalDot = description.endsWith('.') ? description.slice(0, description.length - 1) : description;
  let parsedDescription = ' ' + removedFinalDot + ' ';
  const tokenData = getTokensData(translateData);

  const idsMatched = tokenData.idList.filter(id => parsedDescription.includes(`#${ id }`));
  const tokensMatchedByName = tokenData.nameList.filter(name => parsedDescription.toUpperCase().includes(` ${ name.toUpperCase() }`));
  let tokensMatchedBySymbol = tokenData.symbolList.filter(symbol => parsedDescription.toUpperCase().includes(` ${ symbol.toUpperCase() }`));

  // Filter symbols if they're already matched by name
  tokensMatchedBySymbol = tokensMatchedBySymbol.filter(symbol => !tokensMatchedByName.includes(tokenData.bySymbol[symbol]?.name || ''));

  const summaryValues: Array<SummaryValues> = [];

  if (idsMatched.length) {
    parsedDescription = removeIds(tokensMatchedByName, tokensMatchedBySymbol, idsMatched, tokenData, parsedDescription);
  }

  if (tokensMatchedByName.length) {
    const values = createTokensSummaryValues(tokensMatchedByName, tokenData.byName);
    summaryValues.push(...values);
  }

  if (tokensMatchedBySymbol.length) {
    const values = createTokensSummaryValues(tokensMatchedBySymbol, tokenData.bySymbol);
    summaryValues.push(...values);
  }

  const addressSummaryValues = createAddressValues(translateData, parsedDescription);
  if (addressSummaryValues.length) {
    summaryValues.push(...addressSummaryValues);
  }

  return createSummaryTemplate(summaryValues, parsedDescription) as TxInterpretationSummary;
};

const removeIds = (
  tokensMatchedByName: Array<string>,
  tokensMatchedBySymbol: Array<string>,
  idsMatched: Array<string>,
  tokenData: TokensData,
  parsedDescription: string,
) => {
  // Remove ids from the description since we already have that info in the token object
  let description = parsedDescription;

  tokensMatchedByName.forEach(name => {
    const hasId = idsMatched.includes(tokenData.byName[name].id || '');
    if (hasId) {
      description = description.replaceAll(`#${ tokenData.byName[name].id }`, '');
    }
  });

  tokensMatchedBySymbol.forEach(name => {
    const hasId = idsMatched.includes(tokenData.bySymbol[name].id || '');
    if (hasId) {
      description = description.replaceAll(`#${ tokenData.bySymbol[name].id }`, '');
    }
  });

  return description;
};

const createTokensSummaryValues = (
  matchedStrings: Array<string>,
  tokens: {
    [x: string]: NovesTokenInfo;
  },
) => {
  const summaryValues: Array<SummaryValues> = matchedStrings.map(match => ({
    match,
    type: 'token',
    value: tokens[match],
  }));

  return summaryValues;
};

const createSummaryTemplate = (summaryValues: Array<SummaryValues | undefined>, parsedDescription: string) => {
  let newDescription = parsedDescription;

  const result: NovesSummary = {
    summary_template: newDescription,
    summary_template_variables: {},
  };

  if (!summaryValues[0]) {
    return result;
  }

  const createTemplate = (data: SummaryValues, index = 0) => {
    newDescription = newDescription.replaceAll(new RegExp(` ${ data.match } `, 'gi'), `{${ data.match }}`);

    const variable = {
      type: data.type,
      value: data.value,
    };

    result.summary_template_variables[data.match] = variable;

    const nextValue = summaryValues[index + 1];
    if (nextValue) {
      createTemplate(nextValue, index + 1);
    }
  };

  createTemplate(summaryValues[0]);

  result.summary_template = newDescription;

  return result;
};
