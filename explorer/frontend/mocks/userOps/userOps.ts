import type { UserOpsResponse } from 'types/api/userOps';

export const userOpsData: UserOpsResponse = {
  items: [
    {
      address: {
        ens_domain_name: null,
        hash: '0xF0C14FF4404b188fAA39a3507B388998c10FE284',
        implementations: null,
        is_contract: true,
        is_verified: null,
        name: null,
      },
      block_number: '10399597',
      fee: '187125856691380',
      hash: '0xe72500491b3f2549ac53bd9de9dbb1d2edfc33cdddf5c079d6d64dfec650ef83',
      status: true,
      timestamp: '2022-01-19T12:42:12.000000Z',
      transaction_hash: '0x715fe1474ac7bea3d6f4a03b1c5b6d626675fb0b103be29f849af65e9f1f9c6a',
    },
    {
      address:
        { ens_domain_name: null,
          hash: '0x2c298CcaFFD1549e1C21F46966A6c236fCC66dB2',
          implementations: null,
          is_contract: true,
          is_verified: null,
          name: null,
        },
      block_number: '10399596',
      fee: '381895502291373',
      hash: '0xcb945ae86608bdc88c3318245403c81a880fcb1e49fef18ac59477761c056cea',
      status: false,
      timestamp: '2022-01-19T12:42:00.000000Z',
      transaction_hash: '0x558d699e7cbc235461d48ed04b8c3892d598a4000f20851760d00dc3513c2e48',
    },
    {
      address: {
        ens_domain_name: null,
        hash: '0x2c298CcaFFD1549e1C21F46966A6c236fCC66dB2',
        implementations: null,
        is_contract: true,
        is_verified: null,
        name: null,
      },
      block_number: '10399560',
      fee: '165019501210143',
      hash: '0x84c1270b12af3f0ffa204071f1bf503ebf9b1ccf6310680383be5a2b6fd1d8e5',
      status: true,
      timestamp: '2022-01-19T12:32:00.000000Z',
      transaction_hash: '0xc4c1c38680ec63139411aa2193275e8de44be15217c4256db9473bf0ea2b6264',
    },
  ],
  next_page_params: {
    page_size: 50,
    page_token: '10396582,0x9bf4d2a28813c5c244884cb20cdfe01dabb3f927234ae961eab6e38502de7a28',
  },
};
