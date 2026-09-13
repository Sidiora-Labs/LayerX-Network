import * as React from 'react';
import { PlatformSetting } from '../lib/platform.cjs';
import { ButtonProps } from './button.cjs';
import 'class-variance-authority/types';
import 'class-variance-authority';

/**
 * The screen's one primary CTA, placed per platform contract:
 * - mobile: full-width pill pinned at thumb reach (sticky footer with a
 *   soft gradient scrim above the tab bar / home indicator)
 * - desktop: fixed-width button — render it in the pane footer
 *   (`position="footer"`) or pass it to AppShell `headerActions`
 *   (`position="header"`).
 */
declare function PrimaryAction({ children, platform, position, className, ...props }: ButtonProps & {
    platform?: PlatformSetting;
    position?: "footer" | "header";
}): React.JSX.Element;

export { PrimaryAction };
