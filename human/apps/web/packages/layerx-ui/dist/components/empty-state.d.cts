import * as React from 'react';

/**
 * Centered empty state — soft circular icon well, title, copy, optional CTA.
 * ("Ready to get started?" from the design set.)
 */
declare function EmptyState({ icon, title, description, action, className, }: {
    icon?: React.ReactNode;
    title: React.ReactNode;
    description?: React.ReactNode;
    action?: React.ReactNode;
    className?: string;
}): React.JSX.Element;

export { EmptyState };
