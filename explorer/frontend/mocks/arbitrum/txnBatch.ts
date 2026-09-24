/* eslint-disable max-len */
import type { ArbitrumL2TxnBatch } from 'types/api/arbitrumL2';

import { finalized } from './txnBatches';

export const batchData: ArbitrumL2TxnBatch = {
  ...finalized,
  after_acc_hash: '0xcd064f3409015e8e6407e492e5275a185e492c6b43ccf127f22092d8057a9ffb',
  before_acc_hash: '0x2ed7c4985eb778d76ec400a43805e7feecc8c2afcdb492dbe5caf227de6d37bc',
  start_block_number: 1245209,
  end_block_number: 1245490,
  data_availability: {
    batch_data_container: 'in_blob4844',
  },
};

export const batchDataAnytrust: ArbitrumL2TxnBatch = {
  ...finalized,
  after_acc_hash: '0xcd064f3409015e8e6407e492e5275a185e492c6b43ccf127f22092d8057a9ffb',
  before_acc_hash: '0x2ed7c4985eb778d76ec400a43805e7feecc8c2afcdb492dbe5caf227de6d37bc',
  start_block_number: 1245209,
  end_block_number: 1245490,
  data_availability: {
    batch_data_container: 'in_anytrust',
    bls_signature: '0x142577943e30b1ad1b4e40a1c08e00c24a68d6c366f953e361048b7127e327b5bdb8f168ba986beae40cfaf79ea2788004d750555684751e361d6f6445e5c521b45ac93a76da24add241a4a5410ca3a09fa82cf0aafd78801cbd0ad99d5be6b3',
    data_hash: '0x4ffada101d8185bcba227f2cff9e0ea0a4deeb08f328601a898131429a436ebe',
    timeout: '2024-08-22T12:39:22Z',
    signers: [
      {
        key: '0x0c6694955b524d718ca445831c5375393773401f33725a79661379dddabd5fff28619dc070befd9ed73d699e5c236c1a163be58ba81002b6130709bc064af5d7ba947130b72056bf17263800f1a3ab2269c6a510ef8e7412fd56d1ef1b916a1306e3b1d9c82c099371bd9861582acaada3a16e9dfee5d0ebce61096598a82f112d0a935e8cab5c48d82e3104b0c7ba79157dad1a019a3e7f6ad077b8e6308b116fec0f58239622463c3631fa01e2b4272409215b8009422c16715dbede590906',
        proof: '0x06dcb5e56764bb72e6a45e6deb301ca85d8c4315c1da2efa29927f2ac8fb25571ce31d2d603735fe03196f6d56bcbf9a1999a89a74d5369822c4445d676c15ed52e5008daa775dc9a839c99ff963a19946ac740579874dac4f639907ae1bc69f',
        trusted: false,
      },
      {
        key: '0x0ee5aaeabd57313285207eb89366b411286cf3f1c5e30eb7e355f55385308b91d5807284323ee89a9743c70676f4949504ced3ed41612cbfda06ad55200c1c77d3fb3700059befd64c44bc4a57cb567ec1481ee564cf6cd6cf1f2f4a2dee6db00c547c38400ab118dedae8afd5bab93b703f76a0991baa5d43fbb125194c06b5461f8c738a3c4278a3d98e5456aec0720883c0d28919537a36e2ffd5f731e742b6653557d154c164e068ef983b367ef626faaed46f4eadecbb12b7e55f23175d',
        trusted: true,
      },
    ],
  },
};

export const batchDataCelestia: ArbitrumL2TxnBatch = {
  ...finalized,
  after_acc_hash: '0xcd064f3409015e8e6407e492e5275a185e492c6b43ccf127f22092d8057a9ffb',
  before_acc_hash: '0x2ed7c4985eb778d76ec400a43805e7feecc8c2afcdb492dbe5caf227de6d37bc',
  start_block_number: 1245209,
  end_block_number: 1245490,
  data_availability: {
    batch_data_container: 'in_celestia',
    height: 4520041,
    transaction_commitment: '0x3ebe5a43f47fbf69db003e543bb27e4875929ede2fa9a25d09f0bd082d5d20f0',
  },
};
