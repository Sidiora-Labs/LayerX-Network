"use client";
import {
  cn
} from "./chunk-LXFZWLUU.js";

// src/components/stat.tsx
import { jsx, jsxs } from "react/jsx-runtime";
function Stat({
  value,
  label,
  className,
  align = "center"
}) {
  return /* @__PURE__ */ jsxs(
    "div",
    {
      className: cn(
        "flex flex-col gap-1",
        align === "center" ? "items-center text-center" : "items-start",
        className
      ),
      children: [
        /* @__PURE__ */ jsx("span", { className: "text-2xl font-bold tabular-nums text-foreground", children: value }),
        /* @__PURE__ */ jsx("span", { className: "text-sm text-muted-foreground", children: label })
      ]
    }
  );
}
function StatPair({
  left,
  right,
  className
}) {
  return /* @__PURE__ */ jsxs("div", { className: cn("grid grid-cols-2", className), children: [
    /* @__PURE__ */ jsx(Stat, { value: left.value, label: left.label }),
    /* @__PURE__ */ jsx(Stat, { value: right.value, label: right.label, className: "border-l border-border" })
  ] });
}

export {
  Stat,
  StatPair
};
//# sourceMappingURL=chunk-WIVRKZBR.js.map