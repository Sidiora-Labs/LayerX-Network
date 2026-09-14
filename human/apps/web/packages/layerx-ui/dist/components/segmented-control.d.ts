import * as React from 'react';

interface SegmentedControlOption {
    value: string;
    label: React.ReactNode;
}
interface SegmentedControlProps {
    options: SegmentedControlOption[];
    value: string;
    onValueChange: (value: string) => void;
    className?: string;
    size?: "sm" | "md";
    "aria-label"?: string;
}
/**
 * Gray track + white active thumb — the "Today / This week / This month"
 * recency switcher and "All / Fiat / Crypto" filter in the design set.
 */
declare function SegmentedControl({ options, value, onValueChange, className, size, ...aria }: SegmentedControlProps): React.JSX.Element;

export { SegmentedControl, type SegmentedControlOption, type SegmentedControlProps };
