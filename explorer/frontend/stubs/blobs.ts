import type { Blob, TxBlob } from 'types/api/blobs';

import { TX_HASH } from './tx';

const BLOB_HASH = '0x0137cd898a9aaa92bbe94999d2a98241f5eabc829d9354160061789963f85995';
const BLOB_PROOF = '0x82683d5d6e58a76f2a607b8712cad113621d46cb86a6bcfcffb1e274a70c7308b3243c6075ee22d904fecf8d4c147c6f';

export const TX_BLOB: TxBlob = {
  blob_data: '0x010203040506070809101112',
  hash: BLOB_HASH,
  kzg_commitment: BLOB_PROOF,
  kzg_proof: BLOB_PROOF,
};

export const BLOB: Blob = {
  ...TX_BLOB,
  transaction_hashes: [
    { block_consensus: true, transaction_hash: TX_HASH },
  ],
};
