import type { CustomLinksGroup } from 'types/footerLinks';

export const FOOTER_LINKS: Array<CustomLinksGroup> = [
  {
    title: 'Company',
    links: [
      {
        text: 'Advertise',
        url: 'https://coinzilla.com/',
      },
      {
        text: 'Staking',
        url: '',
      },
      {
        text: 'Contact us',
        url: '',
      },
      {
        text: 'Brand assets',
        url: '',
      },
      {
        text: 'Term of service',
        url: '',
      },
    ],
  },
  {
    title: 'Community',
    links: [
      {
        text: 'API docs',
        url: '',
      },
      {
        text: 'Knowledge base',
        url: '',
      },
      {
        text: 'Network status',
        url: '',
      },
      {
        text: 'Learn Alphabet',
        url: '',
      },
    ],
  },
  {
    title: 'Product',
    links: [
      {
        text: 'Stake Alphabet',
        url: '',
      },
      {
        text: 'Build token',
        url: '',
      },
      {
        text: 'Build DAPPS',
        url: '',
      },
      {
        text: 'NFT marketplace',
        url: '',
      },
      {
        text: 'Become validator',
        url: '',
      },

    ],
  },
  {
    title: 'Partners',
    links: [
      {
        text: 'MetaDock',
        url: 'https://blocksec.com/metadock',
        iconUrl: [
          'http://localhost:3000/mocks/image_s.jpg',
          'http://localhost:3000/mocks/image_svg.svg',
        ],
      },
      {
        text: 'Sourcify',
        url: 'https://sourcify.dev/',
      },
      {
        text: 'DRPC',
        url: 'https://drpc.org?ref=559183',
      },
    ],
  },
];
