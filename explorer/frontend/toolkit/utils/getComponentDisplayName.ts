import type React from 'react';

export default function getComponentDisplayName(type: string | React.JSXElementConstructor<unknown>) {
  if (typeof type === 'string') {
    return;
  }

  if ('displayName' in type) {
    return type.displayName as string;
  }
}
