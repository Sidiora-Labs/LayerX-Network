import * as bens from '@blockscout/bens-types';

const domainTokenA: bens.Token = {
  id: '97352314626701792030827861137068748433918254309635329404916858191911576754327',
  contract_hash: '0x57f1887a8bf19b14fc0df6fd9b2acc9af147ea85',
  type: bens.TokenType.NATIVE_DOMAIN_TOKEN,
};
const domainTokenB = {
  id: '423546333',
  contract_hash: '0x57f1887a8bf19b14fc0df6fd9b2acc9af147ea86',
  type: bens.TokenType.WRAPPED_DOMAIN_TOKEN,
};

export const protocolA: bens.ProtocolInfo = {
  id: 'ens',
  short_name: 'ENS',
  title: 'Ethereum Name Service',
  description: 'The Ethereum Name Service (ENS) is a distributed, open, and extensible naming system based on the Ethereum blockchain.',
  tld_list: [
    'eth',
    'xyz',
  ],
  icon_url: 'https://i.imgur.com/GOfUwCb.jpeg',
  docs_url: 'https://docs.ens.domains/',
  deployment_blockscout_base_url: 'http://localhost:3200/',
};

export const protocolB: bens.ProtocolInfo = {
  id: 'duck',
  short_name: 'DUCK',
  title: 'Duck Name Service',
  description: '"Duck Name Service" is a cutting-edge blockchain naming service, providing seamless naming for crypto and decentralized applications. 🦆',
  tld_list: [
    'duck',
    'quack',
  ],
  icon_url: 'https://localhost:3000/duck.jpg',
  docs_url: 'https://docs.duck.domains/',
  deployment_blockscout_base_url: '',
};

export const ensDomainA: bens.DetailedDomain = {
  id: '0xb140bf9645e54f02ed3c1bcc225566b515a98d1688f10494a5c3bc5b447936a7',
  tokens: [
    domainTokenA,
    domainTokenB,
  ],
  name: 'cat.eth',
  registrant: {
    hash: '0x114d4603199df73e7d157787f8778e21fcd13066',
  },
  resolved_address: {
    hash: '0xfe6ab8a0dafe7d41adf247c210451c264155c9b0',
  },
  owner: {
    hash: '0x114d4603199df73e7d157787f8778e21fcd13066',
  },
  registration_date: '2021-06-27T13:34:44.000Z',
  expiry_date: '2025-03-01T14:20:24.000Z',
  other_addresses: {
    ETH: 'fe6ab8a0dafe7d41adf247c210451c264155c9b0',
    GNO: 'DDAfbb505ad214D7b80b1f830fcCc89B60fb7A83',
    NEAR: 'a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48.factory.bridge.near',
  },
  protocol: protocolA,
  resolver_address: {
    hash: '0xD578780f1dA7404d9CC0eEbC9D684c140CC4b638',
  },
  resolved_with_wildcard: true,
  stored_offchain: true,
  wrapped_owner: {
    hash: '0xD4416b13d2b3a9aBae7AcD5D6C2BbDBE25686401',
  },
};

export const ensDomainB: bens.DetailedDomain = {
  id: '0x632ac7bec8e883416b371b36beaa822f4784208c99d063ee030020e2bd09b885',
  tokens: [ domainTokenA ],
  name: 'kitty.kitty.kitty.cat.eth',
  resolved_address: undefined,
  registrant: {
    hash: '0x114d4603199df73e7d157787f8778e21fcd13066',
  },
  owner: {
    hash: '0x114d4603199df73e7d157787f8778e21fcd13066',
  },
  wrapped_owner: undefined,
  registration_date: '2023-08-13T13:01:12.000Z',
  expiry_date: undefined,
  other_addresses: {},
  protocol: undefined,
  resolved_with_wildcard: false,
  stored_offchain: false,
};

export const ensDomainC: bens.DetailedDomain = {
  id: '0xdb7f351de6d93bda077a9211bdc49f249326d87932e4787d109b0262e9d189ad',
  tokens: [ domainTokenA ],
  name: 'duck.duck.eth',
  registrant: {
    hash: '0x114d4603199df73e7d157787f8778e21fcd13066',
  },
  resolved_address: {
    hash: '0xfe6ab8a0dafe7d41adf247c210451c264155c9b0',
  },
  owner: {
    hash: '0x114d4603199df73e7d157787f8778e21fcd13066',
  },
  wrapped_owner: undefined,
  registration_date: '2022-04-24T07:34:44.000Z',
  expiry_date: '2022-11-01T13:10:36.000Z',
  other_addresses: {},
  protocol: undefined,
  resolved_with_wildcard: false,
  stored_offchain: false,
};

export const ensDomainD: bens.DetailedDomain = {
  id: '0xdb7f351de6d93bda077a9211bdc49f249326d87932e4787d109b0262e9d189ae',
  tokens: [ domainTokenA ],
  name: '🦆.duck.eth',
  registrant: {
    hash: '0x114d4603199df73e7d157787f8778e21fcd13066',
  },
  resolved_address: {
    hash: '0x114d4603199df73e7d157787f8778e21fcd13066',
  },
  owner: undefined,
  wrapped_owner: undefined,
  registration_date: '2022-04-24T07:34:44.000Z',
  expiry_date: '2027-09-23T13:10:36.000Z',
  other_addresses: {},
  protocol: undefined,
  resolved_with_wildcard: false,
  stored_offchain: false,
};
