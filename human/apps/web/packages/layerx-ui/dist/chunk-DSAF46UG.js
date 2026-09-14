"use client";
import {
  cn
} from "./chunk-LXFZWLUU.js";

// src/components/segmented-control.tsx
import { jsx } from "react/jsx-runtime";
function SegmentedControl({
  options,
  value,
  onValueChange,
  className,
  size = "md",
  ...aria
}) {
  return /* @__PURE__ */ jsx(
    "div",
    {
      role: "tablist",
      className: cn(
        "flex w-full items-center rounded-full bg-surface-sunken p-1",
        size === "sm" ? "h-9" : "h-11",
        className
      ),
      ...aria,
      children: options.map((opt) => {
        const active = opt.value === value;
        return /* @__PURE__ */ jsx(
          "button",
          {
            role: "tab",
            "aria-selected": active,
            type: "button",
            onClick: () => onValueChange(opt.value),
            className: cn(
              "flex h-full flex-1 items-center justify-center rounded-full font-semibold whitespace-nowrap transition-all outline-none focus-visible:ring-2 focus-visible:ring-accent/30",
              size === "sm" ? "px-3 text-[13px]" : "px-4 text-sm",
              active ? "bg-surface text-foreground shadow-[0_1px_4px_rgb(0_0_0/0.10)]" : "text-faint-foreground hover:text-muted-foreground"
            ),
            children: opt.label
          },
          opt.value
        );
      })
    }
  );
}

export {
  SegmentedControl
};
//# sourceMappingURL=chunk-DSAF46UG.js.map