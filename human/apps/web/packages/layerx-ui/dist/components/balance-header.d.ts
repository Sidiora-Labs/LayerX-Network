import * as React from 'react';

interface BalanceHeaderProps {
    label?: string;
    value: number;
    symbol?: string;
    /** Daily change info line, e.g. { amount: "$234", percent: "+0.81%", up: true } */
    change?: {
        text: string;
        up: boolean;
    };
    hidden?: boolean;
    onHiddenChange?: (hidden: boolean) => void;
    align?: "left" | "center";
    className?: string;
}
/**
 * Big balance with privacy eye toggle — the home/wallet header
 * in the design set ("$ 23,043.00" + 1-day change).
 */
declare function BalanceHeader({ label, value, symbol, change, hidden: hiddenProp, onHiddenChange, align, className, }: BalanceHeaderProps): React.JSX.Element;

export { BalanceHeader, type BalanceHeaderProps };
