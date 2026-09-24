// https://hexdocs.pm/phoenix/js/
import type { Channel, SocketConnectOption } from 'phoenix';
import { Socket } from 'phoenix';
import React, { useEffect, useState } from 'react';

type ChannelRegistry = Record<string, { channel: Channel; subscribers: number }>;

const socketContexts = new Map<string, React.Context<{
  socket: Socket | null;
  channelRegistry: React.MutableRefObject<ChannelRegistry>;
} | null>>();

function getSocketContext(name: string) {
  if (!socketContexts.has(name)) {
    socketContexts.set(name, React.createContext<{
      socket: Socket | null;
      channelRegistry: React.MutableRefObject<ChannelRegistry>;
    } | null>(null));
  }
  return socketContexts.get(name)!;
}

interface SocketProviderProps {
  children: React.ReactNode;
  url?: string;
  options?: Partial<SocketConnectOption>;
  name?: string;
}

export function SocketProvider({ children, options, url, name = 'default' }: SocketProviderProps) {
  const [ socket, setSocket ] = useState<Socket | null>(null);
  const channelRegistry = React.useRef<ChannelRegistry>({});

  useEffect(() => {
    if (!url) {
      return;
    }

    const socketInstance = new Socket(url, options);
    socketInstance.connect();
    setSocket(socketInstance);

    return () => {
      socketInstance.disconnect();
      setSocket(null);
    };
  }, [ options, url ]);

  const value = React.useMemo(() => ({
    socket,
    channelRegistry,
  }), [ socket, channelRegistry ]);

  const SocketContext = getSocketContext(name);

  return (
    <SocketContext.Provider value={ value }>
      { children }
    </SocketContext.Provider>
  );
}

// Hook to use a specific named socket
export function useSocket(name: string = 'default') {
  const SocketContext = getSocketContext(name);
  const context = React.useContext(SocketContext);
  if (context === undefined) {
    throw new Error(`useSocket must be used within a SocketProvider with name "${ name }"`);
  }
  return context;
}
