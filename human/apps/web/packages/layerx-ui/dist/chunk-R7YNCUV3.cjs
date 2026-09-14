"use strict";Object.defineProperty(exports, "__esModule", {value: true});"use client";


var _chunkMD6ORKN4cjs = require('./chunk-MD6ORKN4.cjs');

// src/components/badge.tsx
var _classvarianceauthority = require('class-variance-authority');
var _jsxruntime = require('react/jsx-runtime');
var badgeVariants = _classvarianceauthority.cva.call(void 0, 
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
  return /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: _chunkMD6ORKN4cjs.cn.call(void 0, badgeVariants({ variant, size, className })), ...props });
}




exports.badgeVariants = badgeVariants; exports.Badge = Badge;
//# sourceMappingURL=chunk-R7YNCUV3.cjs.map