import { decompress } from 'brotli-compress/js';

import type { FlashblockItemApiMegaEth, FlashblockItemApiOptimism } from 'types/api/flashblocks';

export async function parseSocketEventDataOptimism(event: MessageEvent) {
  try {
    const textData = await event.data.text();
    if (textData.trim().startsWith('{')) {
      return JSON.parse(textData) as FlashblockItemApiOptimism;
    } else {
      const arrayBuffer = await event.data.arrayBuffer();
      const u8Data = new Uint8Array(arrayBuffer);
      const decompressedData = Buffer.from(await decompress(u8Data)).toString('utf-8');
      return JSON.parse(decompressedData) as FlashblockItemApiOptimism;
    }
  } catch (error) {
    // eslint-disable-next-line no-console
    console.error(error);
    return;
  }
}

export function parseSocketEventDataMegaEth(event: MessageEvent) {
  try {
    const parsedMessage = JSON.parse(event.data) as { method: string; params: { result: FlashblockItemApiMegaEth } };
    if (parsedMessage.method === 'eth_subscription') {
      return parsedMessage.params.result;
    }
  } catch (error) {
    // eslint-disable-next-line no-console
    console.error(error);
    return;
  }
}
