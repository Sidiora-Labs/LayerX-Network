"use client";
import {
  cn
} from "./chunk-LXFZWLUU.js";

// src/components/badge.tsx
import { cva } from "class-variance-authority";
import { jsx } from "react/jsx-runtime";
var badgeVariants = cva(
  "inline-flex items-center gap-1 rounded-full font-semibold whitespace-nowrap [&_svg]:size-3",
  {
    variants: {
      variant: {
        /** Default gray pill — Active/Inactive, Settled/Pending. */
        neutral: "bg-surface-sunken text-foreground-secondary",
        success: "bg-success-soft text-success",
        destructive: "bg-destructive-soft text-destructive",
        warning: "bg-warning-soft text-warning",
        accent: "bg-accent-soft text-accent-strong",
        outline: "border border-border-strong text-foreground-secondary"
      },
      size: {
        sm: "h-6 px-2.5 text-xs",
        md: "h-7 px-3 text-[13px]"
      }
    },
    defaultVariants: { variant: "neutral", size: "md" }
  }
);
function Badge({ className, variant, size, ...props }) {
  return /* @__PURE__ */ jsx("span", { className: cn(badgeVariants({ variant, size, className })), ...props });
}

export {
  badgeVariants,
  Badge
};
//# sourceMappingURL=chunk-KTG64LT6.js.map