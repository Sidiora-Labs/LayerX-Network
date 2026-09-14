"use client";
import {
  cn
} from "./chunk-LXFZWLUU.js";

// src/components/input.tsx
import * as React from "react";
import { Search, X } from "lucide-react";
import { jsx, jsxs } from "react/jsx-runtime";
var Input = React.forwardRef(
  ({ className, error, leading, trailing, ...props }, ref) => {
    return /* @__PURE__ */ jsxs(
      "div",
      {
        className: cn(
          "flex h-12 items-center gap-2 rounded-md border bg-surface px-4 transition-colors",
          error ? "border-destructive focus-within:ring-2 focus-within:ring-destructive/25" : "border-border focus-within:border-accent focus-within:ring-2 focus-within:ring-accent/20",
          props.disabled && "opacity-50",
          className
        ),
        children: [
          leading,
          /* @__PURE__ */ jsx(
            "input",
            {
              ref,
              className: "h-full w-full min-w-0 bg-transparent text-[15px] text-foreground outline-none placeholder:text-faint-foreground",
              ...props
            }
          ),
          trailing
        ]
      }
    );
  }
);
Input.displayName = "Input";
var SearchInput = React.forwardRef(
  ({ className, value, onClear, ...props }, ref) => {
    const hasValue = value !== void 0 ? String(value).length > 0 : false;
    return /* @__PURE__ */ jsxs(
      "div",
      {
        className: cn(
          "flex h-11 items-center gap-2.5 rounded-full bg-surface border border-border px-4 transition-colors focus-within:border-accent focus-within:ring-2 focus-within:ring-accent/20",
          className
        ),
        children: [
          /* @__PURE__ */ jsx(Search, { className: "size-[18px] shrink-0 text-muted-foreground", "aria-hidden": true }),
          /* @__PURE__ */ jsx(
            "input",
            {
              ref,
              type: "search",
              value,
              className: "h-full w-full min-w-0 bg-transparent text-[15px] text-foreground outline-none placeholder:text-faint-foreground [&::-webkit-search-cancel-button]:hidden",
              ...props
            }
          ),
          hasValue && onClear && /* @__PURE__ */ jsx(
            "button",
            {
              type: "button",
              onClick: onClear,
              "aria-label": "Clear search",
              className: "shrink-0 text-faint-foreground hover:text-muted-foreground",
              children: /* @__PURE__ */ jsx(X, { className: "size-4" })
            }
          )
        ]
      }
    );
  }
);
SearchInput.displayName = "SearchInput";

export {
  Input,
  SearchInput
};
//# sourceMappingURL=chunk-CI7EVM3C.js.map