import type { Channel } from 'phoenix';
import { useEffect, useRef, useState } from 'react';

import { useSocket } from './context';

interface Params {
  topic: string | undefined;
  params?: object;
  isDisabled: boolean;
  onJoin?: (channel: Channel, message: unknown) => void;
  onSocketClose?: () => void;
  onSocketError?: () => void;
  socketName?: string;
}

export default function useSocketChannel({ topic, params, isDisabled, onJoin, onSocketClose, onSocketError, socketName }: Params) {
  const { socket, channelRegistry } = useSocket(socketName) || {};
  const [ channel, setChannel ] = useState<Channel>();
  const onCloseRef = useRef<string>(undefined);
  const onErrorRef = useRef<string>(undefined);

  const onJoinRef = useRef(onJoin);
  onJoinRef.current = onJoin;

  useEffect(() => {
    const cleanUpRefs = () => {
      const refs = [ onCloseRef.current, onErrorRef.current ].filter(Boolean);
      refs.length > 0 && socket?.off(refs);
    };

    if (!isDisabled) {
      onCloseRef.current = onSocketClose && socket?.onClose(onSocketClose);
      onErrorRef.current = onSocketError && socket?.onError(onSocketError);
    } else {
      cleanUpRefs();
    }

    return cleanUpRefs;
  }, [ onSocketClose, onSocketError, socket, isDisabled ]);

  useEffect(() => {
    if (isDisabled && channel) {
      channel.leave();
      setChannel(undefined);
    }
  }, [ channel, isDisabled ]);

  useEffect(() => {
    if (!socket || isDisabled || !topic || !channelRegistry) {
      return;
    }

    let ch: Channel;
    if (channelRegistry.current[topic]) {
      ch = channelRegistry.current[topic].channel;
      channelRegistry.current[topic].subscribers++;
      onJoinRef.current?.(ch, '');
    } else {
      ch = socket.channel(topic);
      channelRegistry.current[topic] = { channel: ch, subscribers: 1 };
      ch.join()
        .receive('ok', (message) => onJoinRef.current?.(ch, message))
        .receive('error', () => {
          onSocketError?.();
        });
    }

    setChannel(ch);

    const currentRegistry = channelRegistry.current;

    return () => {
      if (currentRegistry[topic]) {
        currentRegistry[topic].subscribers > 0 && currentRegistry[topic].subscribers--;
        if (currentRegistry[topic].subscribers === 0) {
          ch.leave();
          delete currentRegistry[topic];
        }
      }

      setChannel(undefined);
    };
  }, [ socket, topic, params, isDisabled, onSocketError, channelRegistry ]);

  return channel;
}
