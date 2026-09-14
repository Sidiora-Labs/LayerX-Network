import * as React from 'react';

interface CodeInputProps {
    length?: number;
    value: string;
    onChange: (value: string) => void;
    onComplete?: (value: string) => void;
    error?: boolean;
    disabled?: boolean;
    autoFocus?: boolean;
    /** Hide the native input — pair with the on-screen Keypad. */
    readOnly?: boolean;
    className?: string;
    "aria-label"?: string;
}
/**
 * Segmented code entry: per-box display, full-code paste, auto-advance,
 * red error state — the 2FA/PIN kit from the design set.
 * A single hidden input drives everything, so paste and IME work everywhere.
 */
declare function CodeInput({ length, value, onChange, onComplete, error, disabled, autoFocus, readOnly, className, ...aria }: CodeInputProps): React.JSX.Element;
/**
 * On-screen numeric keypad (1–9 with T9 letters, "+*#", 0, backspace) —
 * the mobile half of the code-entry kit.
 */
declare function Keypad({ onDigit, onBackspace, className, }: {
    onDigit: (digit: string) => void;
    onBackspace: () => void;
    className?: string;
}): React.JSX.Element;

export { CodeInput, type CodeInputProps, Keypad };
