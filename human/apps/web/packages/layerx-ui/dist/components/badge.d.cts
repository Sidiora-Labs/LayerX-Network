import * as class_variance_authority_types from 'class-variance-authority/types';
import * as React from 'react';
import { VariantProps } from 'class-variance-authority';

declare const badgeVariants: (props?: ({
    variant?: "accent" | "destructive" | "outline" | "neutral" | "success" | "warning" | null | undefined;
    size?: "sm" | "md" | null | undefined;
} & class_variance_authority_types.ClassProp) | undefined) => string;
interface BadgeProps extends React.HTMLAttributes<HTMLSpanElement>, VariantProps<typeof badgeVariants> {
}
declare function Badge({ className, variant, size, ...props }: BadgeProps): React.JSX.Element;

export { Badge, type BadgeProps, badgeVariants };
