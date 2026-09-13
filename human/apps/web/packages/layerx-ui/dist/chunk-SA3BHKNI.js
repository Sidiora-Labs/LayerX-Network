"use client";
import {
  formatBalance
} from "./chunk-WM3FOCWV.js";
import {
  cn
} from "./chunk-LXFZWLUU.js";

// src/components/balance-header.tsx
import * as React from "react";
import { Eye, EyeOff, TrendingUp, TrendingDown } from "lucide-react";
import { jsx, jsxs } from "react/jsx-runtime";
function BalanceHeader({
  label,
  value,
  symbol = "$",
  change,
  hidden: hiddenProp,
  onHiddenChange,
  align = "left",
  className
}) {
  const [internalHidden, setInternalHidden] = React.useState(false);
  const hidden = hiddenProp ?? internalHidden;
  const toggle = () => {
    const next = !hidden;
    setInternalHidden(next);
    onHiddenChange?.(next);
  };
  return /* @__PURE__ */ jsxs("div", { className: cn("flex flex-col gap-1", align === "center" && "items-center", className), children: [
    label && /* @__PURE__ */ jsx("span", { className: "text-sm text-muted-foreground", children: label }),
    /* @__PURE__ */ jsxs("div", { className: "flex items-center gap-2.5", children: [
      /* @__PURE__ */ jsx("span", { className: "text-[32px] leading-none font-extrabold tabular-nums tracking-tight text-foreground", children: hidden ? `${symbol} \u2022\u2022\u2022\u2022\u2022\u2022` : formatBalance(value, symbol) }),
      /* @__PURE__ */ jsx(
        "button",
        {
          type: "button",
          onClick: toggle,
          "aria-label": hidden ? "Show balance" : "Hide balance",
          className: "inline-flex size-7 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-surface-sunken",
          children: hidden ? /* @__PURE__ */ jsx(EyeOff, { className: "size-[18px]" }) : /* @__PURE__ */ jsx(Eye, { className: "size-[18px]" })
        }
      )
    ] }),
    change && !hidden && /* @__PURE__ */ jsxs("span", { className: "flex items-center gap-1.5 text-sm text-muted-foreground", children: [
      "1 day change:",
      /* @__PURE__ */ jsxs(
        "span",
        {
          className: cn(
            "flex items-center gap-1 font-semibold",
            change.up ? "text-success" : "text-destructive"
          ),
          children: [
            change.text,
            change.up ? /* @__PURE__ */ jsx(TrendingUp, { className: "size-4", "aria-hidden": true }) : /* @__PURE__ */ jsx(TrendingDown, { className: "size-4", "aria-hidden": true })
          ]
        }
      )
    ] })
  ] });
}

export {
  BalanceHeader
};
//# sourceMappingURL=chunk-SA3BHKNI.js.map