import * as class_variance_authority_types from 'class-variance-authority/types';
import * as React from 'react';
import { VariantProps } from 'class-variance-authority';

declare const buttonVariants: (props?: ({
    variant?: "link" | "primary" | "secondary" | "soft" | "accent" | "destructive" | "ghost" | null | undefined;
    size?: "sm" | "md" | "lg" | "icon" | null | undefined;
    fullWidth?: boolean | null | undefined;
} & class_variance_authority_types.ClassProp) | undefined) => string;
interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement>, VariantProps<typeof buttonVariants> {
    asChild?: boolean;
    loading?: boolean;
}
declare const Button: React.ForwardRefExoticComponent<ButtonProps & React.RefAttributes<HTMLButtonElement>>;

declare const iconButtonVariants: (props?: ({
    variant?: "primary" | "soft" | "accent" | "ghost" | "outline" | null | undefined;
    size?: "sm" | "md" | "lg" | null | undefined;
} & class_variance_authority_types.ClassProp) | undefined) => string;
interface IconButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement>, VariantProps<typeof iconButtonVariants> {
}
declare const IconButton: React.ForwardRefExoticComponent<IconButtonProps & React.RefAttributes<HTMLButtonElement>>;

export { Button, type ButtonProps, IconButton, type IconButtonProps, buttonVariants, iconButtonVariants };
