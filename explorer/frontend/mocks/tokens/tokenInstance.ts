/* eslint-disable max-len */
import type { TokenInstance } from 'types/api/token';

import * as addressMock from '../address/address';
import * as tokenInfoMock from './tokenInfo';

export const base: TokenInstance = {
  token: tokenInfoMock.tokenInfoERC721a,
  animation_url: null,
  external_app_url: 'https://duck.nft/get-your-duck-today',
  id: '32925298983216553915666621415831103694597106215670571463977478984525997408266',
  image_url: 'https://example.com/image.jpg',
  is_unique: false,
  holder_address_hash: null,
  metadata: {
    attributes: [
      {
        trait_type: 'skin',
        value: '0',
      },
      {
        trait_type: 'eye',
        value: '2',
      },
      {
        trait_type: 'nose',
        value: '6',
      },
      {
        trait_type: 'spectacles',
        value: '4',
      },
      {
        trait_type: 'hair',
        value: '12',
      },
      {
        trait_type: 'shirt',
        value: '1',
      },
      {
        trait_type: 'earrings',
        value: '4',
      },
      {
        trait_type: 'mouth',
        value: '5',
      },
      {
        trait_type: 'eventURL',
        value: 'https://twitter.com/lilnounsdao?s=21&t=xAihrtwPd6avwdsQqeMXCw',
      },
      {
        trait_type: 'p1',
        value: '57775',
      },
      {
        trait_type: 'p2',
        value: '57772',
      },
      {
        display_type: 'number',
        trait_type: 'difficulty',
        value: 84,
      },
      {
        display_type: 'number',
        trait_type: 'items',
        value: 3,
      },
    ],
    description: '**GENESIS #188848**, **22a5f8bbb1602995** :: *84th* generation of *#57772 and #57775* :: **eGenetic Hash Code (eDNA)** = *3c457cc7f60f7853* :: [Click here for full biography.](https://vipsland.com/nft/collections/genesis/188848) :: crafted by [vipsland](https://vipsland.com/)',
    external_url: 'https://vipsland.com/nft/collections/genesis/188848',
    image: 'https://i.seadn.io/gcs/files/1ee1c5e1ead058322615e3206abb8ba3.png?w=500&auto=format',
    name: 'GENESIS #188848, 22a5f8bbb1602995. Blockchain pixel PFP NFT + "on music video" trait inspired by God',
  },
  owner: addressMock.withName,
  thumbnails: null,
};

export const withRichMetadata: TokenInstance = {
  ...base,
  metadata: {
    background_color: '000000',
    chain: 'MATIC',
    chain_address: '0x66edbdb80001da74cbf3e6c01ba91154f6e2fb7c',
    name: 'Carmelo Anthony',
    total_nfts: 0,
    animation_url: 'https://nftu.com/nft-content/media/PAPAYA/92ee5f5c-bce9-4d64-8a25-c7e1e6305572/dee8734bbefb0d63d6156b6fa0e1385822480589daa1862cbd37a94f6bc2ba3a',
    series_key: 'Series',
    nft_id: 'c746af09-8dcb-4cec-aa8a-5ff02fffc3f1',
    description: 'All-Conference and All-American honors await Carmelo Anthony during his Freshman season for Syracuse. However, Anthony must first defeat a worthy opponent in Georgetown with a double-double effort of 30 points and 15 rebounds.\n \n\n© Syracuse University',
    immutable_uri: 'https://nftu.com/nft-content/metadata/PAPAYA/92ee5f5c-bce9-4d64-8a25-c7e1e6305572/7741920',
    contract_address: '0x63cf7b3d5808cb190aa301b55aafd6b4bb95efbb',
    is_pack: false,
    pack_open_locked_until: '2022-03-05T16:58:30.998Z',
    rarity_key: 'Rarity',
    images: {
      png: {
        primary: {
          url: 'https://nftu.com/nft-content/media/PAPAYA/92ee5f5c-bce9-4d64-8a25-c7e1e6305572/0c66645c4e119f9c5def80273b768138d797f00583f557065a50bb0dd491e8e3',
          cid: 'Qmf9hHAP884ZwYngk3VdVU7rhKDToykTy24WmcoegapnG8',
        },
        secondary: {
          more: {
            deeper: {
              jpeg: {
                url: 'https://nftu.com/nft-content/media/PAPAYA/92ee5f5c-bce9-4d64-8a25-c7e1e6305572/0c66645c4e119f9c5def80273b768138d797f00583f557065a50bb0dd491e8e3/pfp_3.png',
              },
            },
          },
        },
      },
      mp4: {
        primary: {
          url: 'https://nftu.com/nft-content/media/PAPAYA/92ee5f5c-bce9-4d64-8a25-c7e1e6305572/dee8734bbefb0d63d6156b6fa0e1385822480589daa1862cbd37a94f6bc2ba3a',
          cid: 'QmPGMksnyQemncHKQ67zGiuTAsnFi8HTJkY9ebQ6eVVQLv',
        },
      },
      'default': 'mp4',
      webp: [
        'QmPGMksnyQemncHKQ67zGiuTAsnFi8HTJkY9ebQ6eVVQLv',
        'https://nftu.com/nft/92ee5f5c-bce9-4d64-8a25-c7e1e6305572/949',
        {
          label: 'fancy label',
          data: [
            {
              name: 'John',
              email: 'john@foo.com',
            },
            {
              name: 'Mary',
              email: 'mary@foo.com',
            },
            [ 1, 2 ],
          ],
        },
        [
          {
            address: 'unknown',
            age: 523,
            gender: 'male',
          },
          {
            address: 'bar',
            age: 24,
            gender: 'https://nftu.com/nft/92ee5f5c-bce9-4d64-8a25-c7e1e6305572/949',
          },
        ],
      ],
    },
    royalty_amount: 1000,
    rarity: 'Premium',
    set_key: 'Set',
    external_url: 'https://nftu.com/nft/92ee5f5c-bce9-4d64-8a25-c7e1e6305572/949',
    attributes: [
      {
        value: 'NCAABB',
        trait_type: 'Sport',
      },
      {
        value: 'Player',
        trait_type: 'Type',
      },
      {
        value: '15',
        trait_type: 'Player Jersey Number',
        display_type: 'number',
      },
    ],
    tags: [ 'foo', 123, true ],
    token_id: '7741920',
    serial_total: 1100,
    blockchain_state: 'BURNING',
    image: 'ipfs://dee8734bbefb0d63d6156b6fa0e1385822480589daa1862cbd37a94f6bc2ba3a',
    revealed_nfts: null,
    nft_data_id: '92ee5f5c-bce9-4d64-8a25-c7e1e6305572',
    series: 'Tip-Off',
    immutable_cid: 'QmVigZH1P3D6QWvp2SWVreTPKmDvUYUidNzcUrcYzATpyJ',
    status: null,
  },
};

export const unique: TokenInstance = {
  ...base,
  is_unique: true,
};
