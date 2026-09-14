import * as React from 'react';
import { PlatformSetting } from '../lib/platform.cjs';

interface NavItem {
    id: string;
    label: string;
    icon?: React.ReactNode;
    /** Numeric badge (e.g. pending approvals on Activity). */
    badge?: number;
}
/**
 * Mobile navigation: bottom tab bar (Home, Agents, Activity, More) with a
 * raised center action button at thumb reach.
 */
declare function BottomTabBar({ items, activeId, onNavigate, onFab, fabIcon, className, }: {
    items: NavItem[];
    activeId: string;
    onNavigate?: (id: string) => void;
    onFab?: () => void;
    fabIcon?: React.ReactNode;
    className?: string;
}): React.JSX.Element;
/**
 * Desktop navigation: left sidebar with product nav (badge support for
 * approvals), an Explorer link, and a settings footer.
 */
declare function Sidebar({ items, activeId, onNavigate, logo, explorerLabel, onExplorer, settingsLabel, onSettings, user, className, }: {
    items: NavItem[];
    activeId: string;
    onNavigate?: (id: string) => void;
    logo?: React.ReactNode;
    explorerLabel?: string;
    onExplorer?: () => void;
    settingsLabel?: string;
    onSettings?: () => void;
    user?: {
        name: string;
        subtitle?: string;
        avatarSrc?: string;
    };
    className?: string;
}): React.JSX.Element;
interface AppShellProps {
    nav: NavItem[];
    activeNav: string;
    onNavigate?: (id: string) => void;
    /** Center action — mobile FAB / desktop header button. */
    onPrimaryAction?: () => void;
    primaryActionLabel?: string;
    primaryActionIcon?: React.ReactNode;
    /** Header slots */
    user?: {
        name: string;
        initials?: string;
        avatarSrc?: string;
    };
    onSearch?: () => void;
    onNotifications?: () => void;
    notificationCount?: number;
    notificationControl?: React.ReactNode;
    /** Desktop sidebar extras */
    onExplorer?: () => void;
    onSettings?: () => void;
    logo?: React.ReactNode;
    /** Page title shown in the desktop header. */
    title?: React.ReactNode;
    /** Desktop header actions (page-level buttons). */
    headerActions?: React.ReactNode;
    platform?: PlatformSetting;
    className?: string;
    children: React.ReactNode;
}
/**
 * The responsive app frame.
 * Mobile → header (avatar, search, bell) + content + bottom tab bar w/ FAB.
 * Desktop → left sidebar (approval badge, Explorer, settings footer) +
 * top header (title, search field, bell, avatar) + content.
 */
declare function AppShell({ nav, activeNav, onNavigate, onPrimaryAction, primaryActionLabel, primaryActionIcon, user, onSearch, onNotifications, notificationCount, notificationControl, onExplorer, onSettings, logo, title, headerActions, platform, className, children, }: AppShellProps): React.JSX.Element;

export { AppShell, type AppShellProps, BottomTabBar, type NavItem, Sidebar };
