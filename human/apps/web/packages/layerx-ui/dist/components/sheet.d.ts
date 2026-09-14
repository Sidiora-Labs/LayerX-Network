import * as React from 'react';

interface SheetProps {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    children: React.ReactNode;
    /** Portal target — pass a phone-frame ref in docs to contain the sheet. */
    portalContainer?: HTMLElement | null;
}
/**
 * Bottom sheet — the mobile overlay of the design set: drag handle,
 * rounded top corners, slides up over a dimmed page.
 * Esc, overlay tap, and the close affordance all dismiss (focus-trapped).
 */
declare function Sheet({ open, onOpenChange, children, portalContainer }: SheetProps): React.JSX.Element;
/** Top grab handle + optional centered title row. */
declare function SheetHeader({ title, className, children, }: {
    title?: React.ReactNode;
    className?: string;
    children?: React.ReactNode;
}): React.JSX.Element;
declare function SheetDescription({ className, ...props }: React.HTMLAttributes<HTMLParagraphElement>): React.JSX.Element;
declare function SheetBody({ className, ...props }: React.HTMLAttributes<HTMLDivElement>): React.JSX.Element;
/**
 * Stacked/paired CTAs pinned to the sheet bottom.
 * Two children render side-by-side (Clear | Apply), one renders full-width.
 */
declare function SheetFooter({ className, ...props }: React.HTMLAttributes<HTMLDivElement>): React.JSX.Element;

export { Sheet, SheetBody, SheetDescription, SheetFooter, SheetHeader, type SheetProps };
