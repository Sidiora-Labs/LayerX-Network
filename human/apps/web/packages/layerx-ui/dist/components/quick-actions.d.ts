import * as React from 'react';

interface QuickAction {
    id: string;
    label: string;
    icon: React.ReactNode;
}
/**
 * Circle icon + label grid ("Send / Receive / Swap / Pay bills").
 * Circles use a hairline border on white, per the design set.
 */
declare function QuickActions({ actions, onAction, className, }: {
    actions: QuickAction[];
    onAction?: (id: string) => void;
    className?: string;
}): React.JSX.Element;

export { type QuickAction, QuickActions };
