import * as React from 'react';

interface AmountTextProps extends React.HTMLAttributes<HTMLSpanElement> {
    value: number;
    currency?: string;
    locale?: string;
    decimals?: number;
    /** "$" by default; pass "" to hide. */
    symbol?: string;
    /**
     * "signed" — positive renders success green with +, negative destructive red.
     * "neutral" — always foreground, sign still shown.
     */
    colorMode?: "signed" | "neutral";
}
/** Signed money text with tabular figures, colored by sign. */
declare function AmountText({ value, currency, locale, decimals, symbol, colorMode, className, ...props }: AmountTextProps): React.JSX.Element;

export { AmountText, type AmountTextProps };
