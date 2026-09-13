import * as React from 'react';

interface OptionListItem {
    value: string;
    label: React.ReactNode;
    description?: React.ReactNode;
}
/**
 * Right-aligned radio rows — the filter sheet option list
 * ("All time / Today / Last 7 days…") from the design set.
 */
declare function OptionList({ items, value, onValueChange, className, "aria-label": ariaLabel, }: {
    items: OptionListItem[];
    value: string;
    onValueChange: (value: string) => void;
    className?: string;
    "aria-label"?: string;
}): React.JSX.Element;

export { OptionList, type OptionListItem };
