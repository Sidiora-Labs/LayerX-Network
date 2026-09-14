import * as React from 'react';

interface DrawerProps {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    children: React.ReactNode;
    portalContainer?: HTMLElement | null;
    /** Width of the right-side panel. Default 420px. */
    width?: number | string;
}
/**
 * Right-side drawer — the desktop detail/education surface.
 * Esc + overlay dismiss, focus-trapped.
 */
declare function Drawer({ open, onOpenChange, children, portalContainer, width }: DrawerProps): React.JSX.Element;
declare function DrawerHeader({ title, description, onClose, className, }: {
    title: React.ReactNode;
    description?: React.ReactNode;
    onClose?: () => void;
    className?: string;
}): React.JSX.Element;
declare function DrawerBody({ className, ...props }: React.HTMLAttributes<HTMLDivElement>): React.JSX.Element;
declare function DrawerFooter({ className, ...props }: React.HTMLAttributes<HTMLDivElement>): React.JSX.Element;

export { Drawer, DrawerBody, DrawerFooter, DrawerHeader, type DrawerProps };
