"use client";
import {
  cn
} from "./chunk-LXFZWLUU.js";

// src/components/button.tsx
import * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cva } from "class-variance-authority";
import { Loader2 } from "lucide-react";
import { jsx, jsxs } from "react/jsx-runtime";
var buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap font-semibold transition-colors select-none outline-none focus-visible:ring-2 focus-visible:ring-accent/40 disabled:pointer-events-none disabled:opacity-40 [&_svg]:pointer-events-none [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        /** Signature solid black pill. */
        primary: "bg-primary text-primary-foreground hover:bg-primary-hover active:bg-primary-hover",
        /** White pill with a hairline border — pairs with primary in footers. */
        secondary: "bg-surface text-foreground border border-border-strong hover:bg-surface-sunken/60",
        /** Light gray filled pill. */
        soft: "bg-surface-sunken text-foreground hover:bg-border/60",
        /** Blue pill — for accent CTAs. */
        accent: "bg-accent text-accent-foreground hover:bg-accent-strong",
        /** Solid red pill for irreversible actions. */
        destructive: "bg-destructive text-destructive-foreground hover:opacity-90",
        /** Borderless. */
        ghost: "text-foreground hover:bg-surface-sunken",
        /** Text-only accent link. */
        link: "text-accent underline-offset-4 hover:underline h-auto px-0"
      },
      size: {
        sm: "h-9 px-4 text-sm rounded-full [&_svg]:size-4",
        md: "h-11 px-6 text-[15px] rounded-full [&_svg]:size-[18px]",
        lg: "h-[52px] px-7 text-base rounded-full [&_svg]:size-5",
        icon: "size-11 rounded-full [&_svg]:size-5"
      },
      fullWidth: {
        true: "w-full",
        false: ""
      }
    },
    defaultVariants: { variant: "primary", size: "md", fullWidth: false }
  }
);
var Button = React.forwardRef(
  ({ className, variant, size, fullWidth, asChild = false, loading, children, disabled, ...props }, ref) => {
    const Comp = asChild ? Slot : "button";
    return /* @__PURE__ */ jsxs(
      Comp,
      {
        ref,
        disabled: disabled || loading,
        className: cn(buttonVariants({ variant, size, fullWidth, className })),
        ...props,
        children: [
          loading && /* @__PURE__ */ jsx(Loader2, { className: "animate-spin", "aria-hidden": true }),
          children
        ]
      }
    );
  }
);
Button.displayName = "Button";
var iconButtonVariants = cva(
  "inline-flex items-center justify-center rounded-full transition-colors outline-none focus-visible:ring-2 focus-visible:ring-accent/40 disabled:pointer-events-none disabled:opacity-40 [&_svg]:size-5",
  {
    variants: {
      variant: {
        /** White circle with hairline border — the design set's header buttons. */
        outline: "bg-surface border border-border text-foreground hover:bg-surface-sunken/60",
        soft: "bg-surface-sunken text-foreground hover:bg-border/60",
        ghost: "text-foreground hover:bg-surface-sunken",
        accent: "bg-accent text-accent-foreground hover:bg-accent-strong",
        primary: "bg-primary text-primary-foreground hover:bg-primary-hover"
      },
      size: {
        sm: "size-9 [&_svg]:size-4",
        md: "size-11",
        lg: "size-14 [&_svg]:size-6"
      }
    },
    defaultVariants: { variant: "outline", size: "md" }
  }
);
var IconButton = React.forwardRef(
  ({ className, variant, size, ...props }, ref) => /* @__PURE__ */ jsx("button", { ref, className: cn(iconButtonVariants({ variant, size, className })), ...props })
);
IconButton.displayName = "IconButton";

export {
  buttonVariants,
  Button,
  iconButtonVariants,
  IconButton
};
//# sourceMappingURL=chunk-4X2DK7Y3.js.map