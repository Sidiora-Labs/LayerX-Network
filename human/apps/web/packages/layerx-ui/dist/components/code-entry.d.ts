import * as React from 'react';
import { PlatformSetting } from '../lib/platform.js';

/**
 * Code / secret entry, per platform:
 * - mobile:  tap-per-box code kit — segmented display driven by the
 *            on-screen Keypad (native keyboard stays down)
 * - desktop: a single segmented input with full-code paste + auto-advance
 *
 * Includes the resend timer row and error copy from the 2FA screens.
 */
declare function CodeEntry({ length, value, onChange, onComplete, error, errorText, resendIn, onResend, platform, className, }: {
    length?: number;
    value: string;
    onChange: (value: string) => void;
    onComplete?: (value: string) => void;
    error?: boolean;
    errorText?: string;
    /** Seconds until resend is allowed; 0/undefined = resend available. */
    resendIn?: number;
    onResend?: () => void;
    platform?: PlatformSetting;
    className?: string;
}): React.JSX.Element;

export { CodeEntry };
