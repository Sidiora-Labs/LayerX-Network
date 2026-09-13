import * as React from 'react';
import { PlatformSetting } from '../lib/platform.cjs';

interface SearchResultItem {
    id: string;
    title: string;
    subtitle?: string;
    icon?: React.ReactNode;
    /** Extra match terms. */
    keywords?: string[];
}
interface SearchResultGroup {
    id: string;
    label: string;
    items: SearchResultItem[];
}
/**
 * Search, per platform:
 * - mobile:  a pushed full-screen search page with autofocus + recents
 * - desktop: a global command bar (binds Cmd+K / Ctrl+K) with type-ahead
 */
declare function GlobalSearch({ open, onOpenChange, groups, onSelect, recents, placeholder, enableHotkey, platform, portalContainer, }: {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    groups: SearchResultGroup[];
    onSelect?: (item: SearchResultItem) => void;
    recents?: SearchResultItem[];
    placeholder?: string;
    /** Bind Cmd+K / Ctrl+K to open. Desktop only. Default true. */
    enableHotkey?: boolean;
    platform?: PlatformSetting;
    portalContainer?: HTMLElement | null;
}): React.JSX.Element;

export { GlobalSearch, type SearchResultGroup, type SearchResultItem };
