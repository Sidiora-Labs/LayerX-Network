import * as React from 'react';

interface ModalProps {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    children: React.ReactNode;
    portalContainer?: HTMLElement | null;
    className?: string;
}
/**
 * Desktop confirmation modal: centered, max 440px, rounded, Esc + overlay
 * dismiss, focus-trapped (Radix Dialog).
 */
declare function Modal({ open, onOpenChange, children, portalContainer, className }: ModalProps): React.JSX.Element;
declare function ModalHeader({ title, description, onClose, className, }: {
    title: React.ReactNode;
    description?: React.ReactNode;
    onClose?: () => void;
    className?: string;
}): React.JSX.Element;
declare function ModalBody({ className, ...props }: React.HTMLAttributes<HTMLDivElement>): React.JSX.Element;
declare function ModalFooter({ className, ...props }: React.HTMLAttributes<HTMLDivElement>): React.JSX.Element;

export { Modal, ModalBody, ModalFooter, ModalHeader, type ModalProps };
