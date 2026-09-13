import * as React from 'react';
import { PlatformSetting } from '../lib/platform.js';

/**
 * Detail / education surface, per platform:
 * - mobile:  bottom sheet (variant="sheet", default) or a pushed full screen
 *            (variant="pushed") with a back header
 * - desktop: right-side drawer (variant="drawer", default) or an inline
 *            expanding section (variant="inline")
 */
declare function DetailDisclosure({ open, onOpenChange, title, children, mobileVariant, desktopVariant, platform, portalContainer, summary, }: {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    title: React.ReactNode;
    children: React.ReactNode;
    mobileVariant?: "sheet" | "pushed";
    desktopVariant?: "drawer" | "inline";
    platform?: PlatformSetting;
    portalContainer?: HTMLElement | null;
    /** For inline variant: the always-visible summary row content. */
    summary?: React.ReactNode;
}): React.JSX.Element | null;

export { DetailDisclosure };
