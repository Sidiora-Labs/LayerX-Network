import React from 'react';

import type { FlashblockItem } from 'types/client/flashblocks';

import config from 'configs/app';
import { SECOND } from 'toolkit/utils/consts';

import { formatFlashblockItemMegaEth, formatFlashblockItemOptimism } from './formatFlashblockItem';
import { parseSocketEventDataMegaEth, parseSocketEventDataOptimism } from './parseSocketEventData';

const flashblocksFeature = config.features.flashblocks;

const MAX_FLASHBLOCKS_COUNT = 50;
const QUEUE_TIME_THRESHOLD = 200;

type Status = 'initial' | 'connected' | 'disconnected' | 'error';

interface ItemsQueue {
  lastTs: number;
  items: Array<FlashblockItem>;
}

export default function useFlashblocksSocketData() {
  const websocketRef = React.useRef<WebSocket | null>(null);
  const isPausedRef = React.useRef(false);
  const subscriptionTimeoutRef = React.useRef(0);
  const itemsQueueRef = React.useRef<ItemsQueue>({
    lastTs: Date.now(),
    items: [],
  });

  const [ items, setItems ] = React.useState<Array<FlashblockItem>>([]);
  const [ itemsNum, setItemsNum ] = React.useState(0);
  const [ newItemsNum, setNewItemsNum ] = React.useState<number | undefined>(undefined);
  const [ txsNum, setTxsNum ] = React.useState(0);
  const [ initialTs, setInitialTs ] = React.useState<number | undefined>(undefined);
  const [ status, setStatus ] = React.useState<Status>('initial');

  const connect = React.useCallback(() => {
    if (!flashblocksFeature.isEnabled) {
      return;
    }

    websocketRef.current = new WebSocket(flashblocksFeature.socketUrl);

    websocketRef.current.onmessage = async(event) => {
      try {
        const newItem = await (async() => {
          switch (flashblocksFeature.type) {
            case 'optimism': {
              const newFlashBlock = await parseSocketEventDataOptimism(event);
              return newFlashBlock ? formatFlashblockItemOptimism(newFlashBlock) : undefined;
            }
            case 'megaEth': {
              const newFlashBlock = parseSocketEventDataMegaEth(event);
              return newFlashBlock ? formatFlashblockItemMegaEth(newFlashBlock) : undefined;
            }
            default:
              return undefined;
          }
        })();

        if (newItem) {
          const now = Date.now();

          if (now - itemsQueueRef.current.lastTs < QUEUE_TIME_THRESHOLD) {
            itemsQueueRef.current.items.unshift(newItem);
          } else {
            const newItems = [ newItem, ...itemsQueueRef.current.items ];
            const newTxsNum = newItems.reduce((acc, item) => acc + item.transactions_count, 0);

            if (isPausedRef.current) {
              setNewItemsNum((prev) => (prev ?? 0) + newItems.length);
            } else {
              setItems((prev) => {
                if (prev.length === 0) {
                  setInitialTs(now);
                }
                return [ ...newItems, ...prev ].slice(0, MAX_FLASHBLOCKS_COUNT);
              });
            }

            setItemsNum((prev) => prev + newItems.length);
            setTxsNum((prev) => prev + newTxsNum);

            itemsQueueRef.current.items = [];
            itemsQueueRef.current.lastTs = now;
          }
        }
      } catch (error) {}
    };

    websocketRef.current.onopen = () => {
      subscriptionTimeoutRef.current = window.setTimeout(() => {
        if (flashblocksFeature.type === 'megaEth') {
          websocketRef.current?.send(JSON.stringify({
            jsonrpc: '2.0',
            method: 'eth_subscribe',
            params: [
              'miniBlocks',
            ],
            id: 1,
          }));
        }
        setStatus('connected');
      }, SECOND);
    };

    websocketRef.current.onerror = () => {
      setStatus('error');
    };

    websocketRef.current.onclose = () => {
      setStatus('disconnected');
    };
  }, []);

  const pause = React.useCallback(() => {
    isPausedRef.current = true;
    setNewItemsNum(undefined);
  }, []);

  const resume = React.useCallback(() => {
    isPausedRef.current = false;
    setNewItemsNum(undefined);
  }, []);

  React.useEffect(() => {
    connect();
    return () => {
      websocketRef.current?.close();
      window.clearTimeout(subscriptionTimeoutRef.current);
    };
  }, [ connect ]);

  return React.useMemo(() => ({
    items,
    pause,
    resume,
    itemsNum,
    newItemsNum,
    initialTs,
    txsNum,
    status,
  }), [ items, pause, resume, itemsNum, newItemsNum, initialTs, txsNum, status ]);
}
