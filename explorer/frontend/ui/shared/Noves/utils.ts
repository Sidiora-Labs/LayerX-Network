import type { NovesResponseData, NovesSentReceived } from 'types/api/noves';

import type { NovesFlowViewItem } from 'ui/tx/assetFlows/utils/generateFlowViewData';

export interface FromToData {
  text: string;
  address: string;
  name?: string | null;
}

export const getFromTo = (txData: NovesResponseData, currentAddress: string): FromToData => {
  const raw = txData.rawTransactionData;
  const sent = txData.classificationData.sent;
  let sentFound: Array<NovesSentReceived> = [];
  if (sent && sent[0]) {
    sentFound = sent
      .filter((sent) => sent.from.address.toLocaleLowerCase() === currentAddress)
      .filter((sent) => sent.to.address);
  }

  const received = txData.classificationData.received;
  let receivedFound: Array<NovesSentReceived> = [];
  if (received && received[0]) {
    receivedFound = received
      .filter((received) => received.to.address?.toLocaleLowerCase() === currentAddress)
      .filter((received) => received.from.address);
  }

  if (sentFound[0] && receivedFound[0]) {
    if (sentFound.length === receivedFound.length) {
      if (raw.toAddress.toLocaleLowerCase() === currentAddress) {
        return { text: 'Received from', address: raw.fromAddress };
      }

      if (raw.fromAddress.toLocaleLowerCase() === currentAddress) {
        return { text: 'Sent to', address: raw.toAddress };
      }
    }
    if (sentFound.length > receivedFound.length) {
      // already filtered if null

      return { text: 'Sent to', address: sentFound[0].to.address! } ;
    } else {
      return { text: 'Received from', address: receivedFound[0].from.address } ;
    }
  }

  if (sent && sentFound[0]) {
    // already filtered if null

    return { text: 'Sent to', address: sentFound[0].to.address! } ;
  }

  if (received && receivedFound[0]) {
    return { text: 'Received from', address: receivedFound[0].from.address };
  }

  if (raw.toAddress && raw.toAddress.toLocaleLowerCase() === currentAddress) {
    return { text: 'Received from', address: raw.fromAddress };
  }

  if (raw.fromAddress && raw.fromAddress.toLocaleLowerCase() === currentAddress) {
    return { text: 'Sent to', address: raw.toAddress };
  }

  if (!raw.toAddress && raw.fromAddress) {
    return { text: 'Received from', address: raw.fromAddress };
  }

  if (!raw.fromAddress && raw.toAddress) {
    return { text: 'Sent to', address: raw.toAddress };
  }

  return { text: 'Sent to', address: currentAddress };
};

export const getFromToValue = (txData: NovesResponseData, currentAddress: string) => {
  const fromTo = getFromTo(txData, currentAddress);

  return fromTo.text.split(' ').shift()?.toLowerCase();
};

export const getActionFromTo = (item: NovesFlowViewItem): FromToData => {
  return {
    text: item.action.flowDirection === 'toRight' ? 'Sent to' : 'Received from',
    address: item.rightActor.address,
    name: item.rightActor.name,
  };
};
