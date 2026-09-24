export interface TxBlob {
  hash: string;
  blob_data: string | null;
  kzg_commitment: string | null;
  kzg_proof: string | null;
}

export type TxBlobs = {
  items: Array<TxBlob>;
  next_page_params: null;
};

export interface Blob extends TxBlob {
  transaction_hashes: Array<{
    block_consensus: boolean;
    transaction_hash: string;
  }>;
}
