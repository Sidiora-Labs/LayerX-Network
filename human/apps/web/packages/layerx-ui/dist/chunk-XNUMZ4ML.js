"use client";
import {
  cn
} from "./chunk-LXFZWLUU.js";

// src/components/popover.tsx
import * as React from "react";
import * as PopoverPrimitive from "@radix-ui/react-popover";
import { jsx } from "react/jsx-runtime";
var Popover = PopoverPrimitive.Root;
var PopoverTrigger = PopoverPrimitive.Trigger;
var PopoverContent = React.forwardRef(({ className, align = "start", sideOffset = 8, ...props }, ref) => /* @__PURE__ */ jsx(PopoverPrimitive.Portal, { children: /* @__PURE__ */ jsx(
  PopoverPrimitive.Content,
  {
    ref,
    align,
    sideOffset,
    className: cn(
      "z-50 w-auto min-w-[220px] rounded-lg border border-border bg-surface p-2 shadow-overlay outline-none",
      "data-[state=open]:animate-fade-in data-[state=closed]:animate-fade-out",
      className
    ),
    ...props
  }
) }));
PopoverContent.displayName = "PopoverContent";

export {
  Popover,
  PopoverTrigger,
  PopoverContent
};
//# sourceMappingURL=chunk-XNUMZ4ML.js.map