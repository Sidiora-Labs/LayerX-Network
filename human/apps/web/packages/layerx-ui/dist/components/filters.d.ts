import * as React from 'react';
import { PlatformSetting } from '../lib/platform.js';
import { DateRange } from 'react-day-picker';

interface FilterDef {
    id: string;
    label: string;
    type: "options" | "date-range";
    options?: {
        value: string;
        label: string;
    }[];
}
type FilterValues = Record<string, string | DateRange | undefined>;
declare function isFilterActive(v: FilterValues[string]): boolean;
/**
 * Filters, per platform:
 * - mobile:  a Filter button opens a sheet with every filter stacked,
 *            Clear + Apply footer (Apply commits a draft state)
 * - desktop: each filter is a chip; its editor is a popover anchored to
 *            the chip (option lists or a calendar range picker)
 */
declare function FilterBar({ filters, values, onChange, platform, portalContainer, className, }: {
    filters: FilterDef[];
    values: FilterValues;
    onChange: (values: FilterValues) => void;
    platform?: PlatformSetting;
    portalContainer?: HTMLElement | null;
    className?: string;
}): React.JSX.Element;

export { FilterBar, type FilterDef, type FilterValues, isFilterActive };
