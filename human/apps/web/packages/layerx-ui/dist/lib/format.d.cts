/** Money + number formatting helpers used across LayerX UI. */
interface FormatMoneyOptions {
    /** ISO-style currency symbol/code shown after the amount, e.g. "USD". */
    currency?: string;
    /** Force a leading + for positive values. Default true when `signed`. */
    signed?: boolean;
    /** Fraction digits. Default 2. */
    decimals?: number;
    /** Symbol prepended to the number. Default "$". Pass "" for none. */
    symbol?: string;
    /** BCP 47 locale used for separators and digit grouping. */
    locale?: string;
}
declare function formatMoney(value: number, opts?: FormatMoneyOptions): string;
/** "$23,043.00" — unsigned, for balances. */
declare function formatBalance(value: number, symbol?: string): string;
/** Compact "30m ago" / "2h ago" style recency labels. */
declare function formatRecency(date: Date, now?: Date): string;
/** Group key for month bands, e.g. "February 2025". */
declare function monthBandLabel(date: Date): string;
/** Build a CSV string from rows of primitive cells and trigger a download. */
declare function downloadCsv(filename: string, header: string[], rows: (string | number)[][]): void;

export { type FormatMoneyOptions, downloadCsv, formatBalance, formatMoney, formatRecency, monthBandLabel };
