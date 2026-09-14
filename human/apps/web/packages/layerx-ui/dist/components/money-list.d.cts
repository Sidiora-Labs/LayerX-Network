import * as React from 'react';
import { PlatformSetting } from '../lib/platform.cjs';
import { MoneyGroup, MoneyItem } from '../lib/types.cjs';

/**
 * Lists of money, per platform:
 * - mobile:  stacked rows under month bands with subtotals
 * - desktop: a true table — sortable columns, hover states, sticky group
 *            rows, CSV export
 */
declare function MoneyList({ groups, onItemClick, platform, exportName, className, }: {
    groups: MoneyGroup[];
    onItemClick?: (item: MoneyItem) => void;
    platform?: PlatformSetting;
    exportName?: string;
    className?: string;
}): React.JSX.Element;

export { MoneyList };
