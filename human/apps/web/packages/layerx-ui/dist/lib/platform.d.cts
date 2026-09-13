import * as React from 'react';

type Platform = "mobile" | "desktop";
type PlatformSetting = Platform | "auto";
/**
 * Controls how LayerX responsive patterns resolve their mobile/desktop
 * variants. "auto" (default) follows the viewport (mobile = <768px).
 * Wrap demos in a fixed value to force a variant regardless of viewport —
 * e.g. inside a phone frame on a desktop docs page.
 */
declare function PlatformProvider({ value, children, }: {
    value?: PlatformSetting;
    children: React.ReactNode;
}): React.JSX.Element;
/** SSR-safe media query hook (mobile = viewport < 768px). */
declare function useMediaQuery(query: string): boolean;
/**
 * Resolve the current platform. Priority:
 * explicit prop > nearest PlatformProvider > viewport media query.
 */
declare function usePlatform(override?: PlatformSetting): Platform;
/** Renders one of two branches by platform. */
declare function PlatformSwitch({ mobile, desktop, platform, }: {
    mobile: React.ReactNode;
    desktop: React.ReactNode;
    platform?: PlatformSetting;
}): React.JSX.Element;

export { type Platform, PlatformProvider, type PlatformSetting, PlatformSwitch, useMediaQuery, usePlatform };
