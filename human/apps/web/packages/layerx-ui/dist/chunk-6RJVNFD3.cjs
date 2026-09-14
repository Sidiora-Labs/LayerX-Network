"use strict";Object.defineProperty(exports, "__esModule", {value: true});"use client";


var _chunkW6TE4RURcjs = require('./chunk-W6TE4RUR.cjs');


var _chunkMD6ORKN4cjs = require('./chunk-MD6ORKN4.cjs');

// src/components/amount.tsx
var _jsxruntime = require('react/jsx-runtime');
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
  return /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
    "span",
    {
      className: _chunkMD6ORKN4cjs.cn.call(void 0,
        "font-semibold tabular-nums",
        colorMode === "signed" && (value > 0 ? "text-success" : value < 0 ? "text-destructive" : "text-foreground"),
        colorMode === "neutral" && "text-foreground",
        className
      ),
      ...props,
      children: _chunkW6TE4RURcjs.formatMoney.call(void 0, value, { currency, decimals, locale, symbol })
    }
  );
}



exports.AmountText = AmountText;
//# sourceMappingURL=chunk-6RJVNFD3.cjs.map