"use client";
import {
  formatMoney
} from "./chunk-WM3FOCWV.js";
import {
  cn
} from "./chunk-LXFZWLUU.js";

// src/components/amount.tsx
import { jsx } from "react/jsx-runtime";
function AmountText({
  value,
  currency,
  locale,
  decimals,
  symbol,
  colorMode = "signed",
  className,
  ...props
}) {
  return /* @__PURE__ */ jsx(
    "span",
    {
      className: cn(
        "font-semibold tabular-nums",
        colorMode === "signed" && (value > 0 ? "text-success" : value < 0 ? "text-destructive" : "text-foreground"),
        colorMode === "neutral" && "text-foreground",
        className
      ),
      ...props,
      children: formatMoney(value, { currency, decimals, locale, symbol })
    }
  );
}

export {
  AmountText
};
//# sourceMappingURL=chunk-R5UMX4ZN.js.map