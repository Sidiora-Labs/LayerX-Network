import type * as multichain from '@blockscout/multichain-aggregator-types';

import { chainA, chainB, chainC, chainD, chainE } from './chains';

export const domainProtocols: Array<multichain.ProtocolInfo> = [
  {
    id: 'goose',
    short_name: 'GOOSE',
    title: 'Goose Name Service',
    description: 'The Goose Name Service is a decentralized naming system for the Goose blockchain.',
    deployment_blockscout_base_url: 'https://blockscout.com',
    tld_list: [ 'goose' ],
    icon_url: 'https://goose.com/icon.jpg',
    docs_url: 'https://goose.com/docs',
  },
  {
    id: 'duck',
    short_name: 'DUCK',
    title: 'Duck Name Service',
    description: 'The Duck Name Service is a decentralized naming system for the Duck blockchain.',
    deployment_blockscout_base_url: 'https://blockscout.com',
    tld_list: [ 'duck', 'quack' ],
    icon_url: 'https://duck.com/icon.jpg',
    docs_url: 'https://duck.com/docs',
  },
];

export const addressA: multichain.GetAddressResponse = {
  hash: '0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045',
  chain_infos: {
    [chainA.id]: {
      coin_balance: '33298149965862412288',
      is_contract: false,
      is_verified: false,
      contract_name: undefined,
    },
    [chainB.id]: {
      coin_balance: '1836931848855642237',
      is_contract: true,
      is_verified: false,
      contract_name: undefined,
    },
    [chainC.id]: {
      coin_balance: '0',
      is_contract: true,
      is_verified: true,
      contract_name: 'Test Contract',
    },
    [chainD.id]: {
      coin_balance: '0',
      is_contract: false,
      is_verified: false,
      contract_name: undefined,
    },
    [chainE.id]: {
      coin_balance: '0',
      is_contract: false,
      is_verified: false,
      contract_name: undefined,
    },
  },
  has_tokens: true,
  has_interop_message_transfers: false,
  coin_balance: '35135081814718054525',
  exchange_rate: '123.455',
  domains: [
    { name: 'ga-ga-ga.goose', protocol: domainProtocols[0] },
    { name: 'quack-quack.duck', protocol: domainProtocols[1] },
  ],
};

export const addressDomainsA: multichain.LookupAddressDomainsResponse = {
  items: [
    { name: 'ga-ga-ga.goose', protocol: domainProtocols[0] },
    { name: 'quack-quack.duck', protocol: domainProtocols[1] },
  ],
  next_page_params: undefined,
};
