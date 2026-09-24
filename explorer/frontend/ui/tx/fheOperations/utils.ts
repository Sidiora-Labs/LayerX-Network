import type { FheOperationType } from 'types/api/fheOperations';

import type { BadgeProps } from 'toolkit/chakra/badge';

// Maps FHE operation types to Blockscout color palette
export function getTypeColor(type: FheOperationType): BadgeProps['colorPalette'] {
  const colors: Record<FheOperationType, BadgeProps['colorPalette']> = {
    comparison: 'purple',
    control: 'orange',
    arithmetic: 'blue',
    bitwise: 'teal',
    encryption: 'cyan',
    unary: 'yellow',
    random: 'pink',
  };
  return colors[type] || 'gray';
}
