import * as React from 'react';

/** Vertical stack of rows with hairline dividers between them. */
declare function List({ className, ...props }: React.HTMLAttributes<HTMLDivElement>): React.JSX.Element;
/** Soft rounded-square icon container (quick actions, list leading icons). */
declare function IconTile({ className, tone, shape, ...props }: React.HTMLAttributes<HTMLSpanElement> & {
    tone?: "neutral" | "accent" | "success" | "destructive";
    shape?: "square" | "circle";
}): React.JSX.Element;
interface ListItemProps extends Omit<React.HTMLAttributes<HTMLDivElement>, "title"> {
    /** Avatar, IconTile, flag, or any leading node. */
    leading?: React.ReactNode;
    title: React.ReactNode;
    subtitle?: React.ReactNode;
    /** Amount, Badge, Switch, chevron… right-aligned. */
    trailing?: React.ReactNode;
    /** Adds a chevron and pointer cursor. */
    navigates?: boolean;
    /** Text under the trailing node (e.g. timestamp). */
    trailingCaption?: React.ReactNode;
}
declare function ListItem({ className, leading, title, subtitle, trailing, trailingCaption, navigates, onClick, ...props }: ListItemProps): React.JSX.Element;
declare function SectionHeader({ title, action, className, }: {
    title: React.ReactNode;
    /** e.g. a "View all" chip button. */
    action?: React.ReactNode;
    className?: string;
}): React.JSX.Element;
/** Small pill button used for "View all" actions in section headers. */
declare function ViewAllChip({ className, children, ...props }: React.ButtonHTMLAttributes<HTMLButtonElement>): React.JSX.Element;
declare function Divider({ className }: {
    className?: string;
}): React.JSX.Element;

export { Divider, IconTile, List, ListItem, type ListItemProps, SectionHeader, ViewAllChip };
