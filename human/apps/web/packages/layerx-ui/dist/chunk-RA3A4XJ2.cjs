"use strict";Object.defineProperty(exports, "__esModule", {value: true}); function _interopRequireWildcard(obj) { if (obj && obj.__esModule) { return obj; } else { var newObj = {}; if (obj != null) { for (var key in obj) { if (Object.prototype.hasOwnProperty.call(obj, key)) { newObj[key] = obj[key]; } } } newObj.default = obj; return newObj; } }"use client";


var _chunkMD6ORKN4cjs = require('./chunk-MD6ORKN4.cjs');

// src/components/button.tsx
var _react = require('react'); var React = _interopRequireWildcard(_react);
var _reactslot = require('@radix-ui/react-slot');
var _classvarianceauthority = require('class-variance-authority');
var _lucidereact = require('lucide-react');
var _jsxruntime = require('react/jsx-runtime');
var buttonVariants = _classvarianceauthority.cva.call(void 0, 
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
    const Comp = asChild ? _reactslot.Slot : "button";
    return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, 
      Comp,
      {
        ref,
        disabled: disabled || loading,
        className: _chunkMD6ORKN4cjs.cn.call(void 0, buttonVariants({ variant, size, fullWidth, className })),
        ...props,
        children: [
          loading && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.Loader2, { className: "animate-spin", "aria-hidden": true }),
          children
        ]
      }
    );
  }
);
Button.displayName = "Button";
var iconButtonVariants = _classvarianceauthority.cva.call(void 0, 
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
  ({ className, variant, size, ...props }, ref) => /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "button", { ref, className: _chunkMD6ORKN4cjs.cn.call(void 0, iconButtonVariants({ variant, size, className })), ...props })
);
IconButton.displayName = "IconButton";






exports.buttonVariants = buttonVariants; exports.Button = Button; exports.iconButtonVariants = iconButtonVariants; exports.IconButton = IconButton;
//# sourceMappingURL=chunk-RA3A4XJ2.cjs.map