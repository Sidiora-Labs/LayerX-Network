import React from 'react';

import type { UptimeHistoryFull, UptimeRealTimeData, UptimeSocketData } from 'types/api/megaEth';

import config from 'configs/app';
import { SECOND } from 'toolkit/utils/consts';

const megaEthFeature = config.features.megaEth;

export type Status = 'initial' | 'connected' | 'disconnected' | 'error';

export default function useUptimeSocketData() {
  const websocketRef = React.useRef<WebSocket | null>(null);
  const heartbeatRef = React.useRef<number | undefined>(undefined);
  const [ status, setStatus ] = React.useState<Status>('initial');
  const [ realtimeData, setRealtimeData ] = React.useState<UptimeRealTimeData | null>(null);
  const [ historyData, setHistoryData ] = React.useState<UptimeHistoryFull | null>(null);

  const connect = React.useCallback(() => {
    if (!megaEthFeature.isEnabled || !megaEthFeature.socketUrl.metrics) {
      return;
    }

    websocketRef.current = new WebSocket(megaEthFeature.socketUrl.metrics);

    websocketRef.current.onmessage = async(event) => {
      try {
        const data = JSON.parse(event.data) as UptimeSocketData;
        switch (data.type) {
          case 'realtime_metrics':
            setRealtimeData(data.realtime);
            break;
          case 'history_full':
            setHistoryData(data.data);
            break;
          case 'history_delta':
            setHistoryData((prev) => {
              if (!prev) {
                return null;
              }

              return Object.entries(data.data).reduce((acc, [ key, value ]) => {
                acc[key as keyof UptimeHistoryFull] = [ ...(acc[key as keyof UptimeHistoryFull] || []), value ];

                return acc;
              }, { ...prev });
            });
            break;
        }
      } catch (error) {
        // eslint-disable-next-line no-console
        console.error(error);
      }
    };

    websocketRef.current.onopen = () => {
      setStatus('connected');

      heartbeatRef.current = window.setInterval(() => {
        if (websocketRef.current?.readyState === WebSocket.OPEN) {
          websocketRef.current?.send('ping');
        }
      }, 10 * SECOND);
    };
    websocketRef.current.onerror = (error) => {
      // eslint-disable-next-line no-console
      console.error(error);
      setStatus('error');
    };
    websocketRef.current.onclose = (event) => {
      // eslint-disable-next-line no-console
      console.error('WebSocket closed', event);
      setStatus('disconnected');
    };
  }, []);

  const onReconnect = React.useCallback(() => {
    connect();
  }, [ connect ]);

  React.useEffect(() => {
    connect();

    return () => {
      websocketRef.current?.close(4000, 'Component unmounted');
      window.clearInterval(heartbeatRef.current);
    };
  }, [ connect ]);

  return { status, realtimeData, historyData, onReconnect };
}
