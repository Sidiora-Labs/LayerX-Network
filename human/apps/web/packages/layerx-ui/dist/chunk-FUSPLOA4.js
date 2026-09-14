"use client";
import {
  cn
} from "./chunk-LXFZWLUU.js";

// src/components/quick-actions.tsx
import { jsx, jsxs } from "react/jsx-runtime";
function QuickActions({
  actions,
  onAction,
  className
}) {
  return /* @__PURE__ */ jsx(
    "div",
    {
      className: cn("grid gap-2", className),
      style: { gridTemplateColumns: `repeat(${Math.min(actions.length, 5)}, minmax(0, 1fr))` },
      children: actions.map((a) => /* @__PURE__ */ jsxs(
        "button",
        {
          type: "button",
          onClick: () => onAction?.(a.id),
          className: "group flex flex-col items-center gap-2 outline-none",
          children: [
            /* @__PURE__ */ jsx("span", { className: "inline-flex size-12 items-center justify-center rounded-full border border-border bg-surface text-foreground transition-colors group-hover:bg-surface-sunken group-focus-visible:ring-2 group-focus-visible:ring-accent/30 [&_svg]:size-5", children: a.icon }),
            /* @__PURE__ */ jsx("span", { className: "text-[13px] font-medium text-foreground-secondary", children: a.label })
          ]
        },
        a.id
      ))
    }
  );
}

export {
  QuickActions
};
//# sourceMappingURL=chunk-FUSPLOA4.js.map