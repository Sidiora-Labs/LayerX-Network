import type { NovesResponseData } from 'types/api/noves';

import type { SummaryAddress, SummaryValues } from './createNovesSummaryObject';

const ADDRESS_REGEXP = /(0x[\da-f]+\b)/gi;
// eslint-disable-next-line regexp/no-unused-capturing-group
const CONTRACT_REGEXP = /(contract 0x[\da-f]+\b)/gi;

export const createAddressValues = (translateData: NovesResponseData, description: string) => {
  const addressMatches = description.match(ADDRESS_REGEXP);
  const contractMatches = description.match(CONTRACT_REGEXP);

  let descriptionAddresses: Array<string> = addressMatches ? addressMatches : [];
  let contractAddresses: Array<string> = [];

  if (contractMatches?.length) {
    contractAddresses = contractMatches.map(text => text.split(ADDRESS_REGEXP)[1]);
    descriptionAddresses = addressMatches?.filter(address => !contractAddresses.includes(address)) || [];
  }

  const addresses = extractAddresses(translateData);

  const descriptionSummaryValues = createAddressSummaryValues(descriptionAddresses, addresses);
  const contractSummaryValues = createAddressSummaryValues(contractAddresses, addresses, true);

  const summaryValues = [ ...descriptionSummaryValues, ...contractSummaryValues ];

  return summaryValues;
};

const createAddressSummaryValues = (descriptionAddresses: Array<string>, addresses: Array<SummaryAddress>, isContract = false) => {
  const summaryValues: Array<SummaryValues | undefined> = descriptionAddresses.map(match => {
    const address = addresses.find(address => address.hash.toUpperCase().startsWith(match.toUpperCase()));

    if (!address) {
      return undefined;
    }

    const value: SummaryValues = {
      match: match,
      type: 'address',
      value: isContract ? { ...address, is_contract: true } : address,
    };

    return value;
  });

  return summaryValues.filter(value => value !== undefined) as Array<SummaryValues>;
};

function extractAddresses(data: NovesResponseData) {
  const addressesSet: Set<{ hash: string | null; name?: string | null }> = new Set(); // Use a Set to store unique addresses

  addressesSet.add({ hash: data.rawTransactionData.fromAddress });
  addressesSet.add({ hash: data.rawTransactionData.toAddress });

  if (data.classificationData.approved) {
    addressesSet.add({ hash: data.classificationData.approved.spender });
  }

  if (data.classificationData.deployedContractAddress) {
    addressesSet.add({ hash: data.classificationData.deployedContractAddress });
  }

  if (data.txTypeVersion === 2) {
    data.classificationData.sent.forEach((transaction) => {
      addressesSet.add({ hash: transaction.from.address, name: transaction.from.name });
      addressesSet.add({ hash: transaction.to.address, name: transaction.to.name });
    });

    data.classificationData.received.forEach((transaction) => {
      addressesSet.add({ hash: transaction.from.address, name: transaction.from.name });
      addressesSet.add({ hash: transaction.to.address, name: transaction.to.name });
    });
  }

  const addresses = Array.from(addressesSet) as Array<{ hash: string; name?: string }>; // Convert Set to an array

  // Remove empty and null values
  return addresses.filter(address => address.hash !== null && address.hash !== '' && address.hash !== undefined);
}
