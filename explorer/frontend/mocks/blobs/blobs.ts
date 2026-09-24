import type { Blob, TxBlobs } from 'types/api/blobs';

export const base1: Blob = {
  blob_data: '0x004242004242004242004242004242004242',
  hash: '0x016316f61a259aa607096440fc3eeb90356e079be01975d2fb18347bd50df33c',
  kzg_commitment: '0xa95caabd009e189b9f205e0328ff847ad886e4f8e719bd7219875fbb9688fb3fbe7704bb1dfa7e2993a3dea8d0cf767d',
  kzg_proof: '0x89cf91c4c8be6f2a390d4262425f79dffb74c174fb15a210182184543bf7394e5a7970a774ee8e0dabc315424c22df0f',
  transaction_hashes: [
    { block_consensus: true, transaction_hash: '0x970d8c45c713a50a1fa351b00ca29a8890cac474c59cc8eee4eddec91a1729f0' },
  ],
};

export const base2: Blob = {
  blob_data: '0x89504E470D0A1A0A0000000D494844520000003C0000003C0403',
  hash: '0x0197fdb17195c176b23160f335daabd4b6a231aaaadd73ec567877c66a3affd1',
  kzg_commitment: '0x89b0d8ac715ee134135471994a161ef068a784f51982fcd7161aa8e3e818eb83017ccfbfc30c89b796a2743d77554e2f',
  kzg_proof: '0x8255a6c6a236483814b8e68992e70f3523f546866a9fed6b8e0ecfef314c65634113b8aa02d6c5c6e91b46e140f17a07',
  transaction_hashes: [
    { block_consensus: true, transaction_hash: '0x22d597ebcf3e8d60096dd0363bc2f0f5e2df27ba1dacd696c51aa7c9409f3193' },
  ],
};

export const withoutData: Blob = {
  blob_data: null,
  hash: '0x0197fdb17195c176b23160f335daabd4b6a231aaaadd73ec567877c66a3affd3',
  kzg_commitment: null,
  kzg_proof: null,
  transaction_hashes: [
    { block_consensus: true, transaction_hash: '0x22d597ebcf3e8d60096dd0363bc2f0f5e2df27ba1dacd696c51aa7c9409f3193' },
  ],
};

export const txBlobs: TxBlobs = {
  items: [ base1, base2, withoutData ],
  next_page_params: null,
};
