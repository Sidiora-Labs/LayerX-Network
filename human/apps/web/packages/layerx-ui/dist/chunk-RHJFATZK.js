"use client";
import {
  cn
} from "./chunk-LXFZWLUU.js";

// src/components/switch.tsx
import * as React from "react";
import * as SwitchPrimitive from "@radix-ui/react-switch";
import { jsx } from "react/jsx-runtime";
var Switch = React.forwardRef(({ className, ...props }, ref) => /* @__PURE__ */ jsx(
  SwitchPrimitive.Root,
  {
    ref,
    className: cn(
      "peer inline-flex h-[26px] w-[46px] shrink-0 cursor-pointer items-center rounded-full border border-transparent transition-colors outline-none",
      "bg-border-strong data-[state=checked]:bg-accent",
      "focus-visible:ring-2 focus-visible:ring-accent/30 disabled:cursor-not-allowed disabled:opacity-50",
      className
    ),
    ...props,
    children: /* @__PURE__ */ jsx(
      SwitchPrimitive.Thumb,
      {
        className: cn(
          "pointer-events-none block size-[22px] rounded-full bg-white shadow-sm ring-0 transition-transform",
          "translate-x-[2px] data-[state=checked]:translate-x-[22px]"
        )
      }
    )
  }
));
Switch.displayName = "Switch";

export {
  Switch
};
//# sourceMappingURL=chunk-RHJFATZK.js.map