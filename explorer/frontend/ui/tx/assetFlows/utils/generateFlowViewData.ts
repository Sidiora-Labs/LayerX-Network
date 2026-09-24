import type { NovesNft, NovesResponseData, NovesSentReceived, NovesToken } from 'types/api/noves';

export interface NovesAction {
  label: string;
  amount: string | undefined;
  flowDirection: 'toLeft' | 'toRight';
  nft: NovesNft | undefined;
  token: NovesToken | undefined;
}

export interface NovesFlowViewItem {
  action: NovesAction;
  rightActor: {
    address: string ;
    name: string | null;
  };
  accountAddress: string;
}

export function generateFlowViewData(data: NovesResponseData): Array<NovesFlowViewItem> {
  const perspectiveAddress = data.accountAddress.toLowerCase();

  const sent = data.classificationData.sent || [];
  const received = data.classificationData.received || [];

  const txItems = [ ...sent, ...received ];

  const paidGasIndex = txItems.findIndex((item) => item.action === 'paidGas');
  if (paidGasIndex >= 0) {
    const element = txItems.splice(paidGasIndex, 1)[0];
    element.to.name = 'Validators';
    txItems.splice(txItems.length, 0, element);
  }

  const flowViewData = txItems.map((item) => {
    const action = {
      label: item.actionFormatted || item.action,
      amount: item.amount || undefined,
      flowDirection: getFlowDirection(item, perspectiveAddress),
      nft: item.nft || undefined,
      token: item.token || undefined,
    };

    if (item.from.name && item.from.name.includes('(this wallet)')) {
      item.from.name = item.from.name.split('(this wallet)')[0];
    }

    if (item.to.name && item.to.name.includes('(this wallet)')) {
      item.to.name = item.to.name.split('(this wallet)')[0];
    }

    const rightActor = getRightActor(item, perspectiveAddress);

    return { action, rightActor, accountAddress: perspectiveAddress };
  });

  return flowViewData;
}

function getRightActor(item: NovesSentReceived, perspectiveAddress: string) {
  if (!item.to.address || item.to.address.toLowerCase() !== perspectiveAddress) {
    return { address: item.to.address || '', name: item.to.name };
  }

  return { address: item.from.address, name: item.from.name };
}

function getFlowDirection(item: NovesSentReceived, perspectiveAddress: string): 'toLeft' | 'toRight' {
  if (item.from.address && item.from.address.toLowerCase() === perspectiveAddress) {
    return 'toRight';
  }

  return 'toLeft';
}
