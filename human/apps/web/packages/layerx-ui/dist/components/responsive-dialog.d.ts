import * as React from 'react';
import { PlatformSetting } from '../lib/platform.js';
import { ButtonProps } from './button.js';
import 'class-variance-authority/types';
import 'class-variance-authority';

/**
 * One overlay, two bodies: bottom sheet on mobile, centered 440px modal on
 * desktop. This is the LayerX confirmation/detail contract from the pattern
 * table — consequence copy included, Esc/overlay dismiss, focus-trapped.
 */
declare function ResponsiveDialog({ open, onOpenChange, title, description, children, footer, platform, portalContainer, }: {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    title: React.ReactNode;
    /** Consequence copy — shown under the title. */
    description?: React.ReactNode;
    children?: React.ReactNode;
    footer?: React.ReactNode;
    platform?: PlatformSetting;
    portalContainer?: HTMLElement | null;
}): React.JSX.Element;
interface ConfirmAction {
    label: React.ReactNode;
    onClick?: () => void;
    variant?: ButtonProps["variant"];
    loading?: boolean;
}
/**
 * Ready-made confirm: icon/title, consequence copy, and paired actions
 * (secondary | primary). Renders as a bottom sheet on mobile, a centered
 * modal on desktop.
 */
declare function ConfirmDialog({ open, onOpenChange, icon, title, consequence, confirm, cancel, platform, portalContainer, }: {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    icon?: React.ReactNode;
    title: React.ReactNode;
    /** The "what happens if you do this" copy. Required — it's the point. */
    consequence: React.ReactNode;
    confirm: ConfirmAction;
    cancel?: ConfirmAction;
    platform?: PlatformSetting;
    portalContainer?: HTMLElement | null;
}): React.JSX.Element;

export { type ConfirmAction, ConfirmDialog, ResponsiveDialog };
