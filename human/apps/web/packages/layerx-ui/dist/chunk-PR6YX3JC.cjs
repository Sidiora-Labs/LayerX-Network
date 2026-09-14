"use strict";Object.defineProperty(exports, "__esModule", {value: true});"use client";


var _chunkMD6ORKN4cjs = require('./chunk-MD6ORKN4.cjs');

// src/components/stat.tsx
var _jsxruntime = require('react/jsx-runtime');
function Stat({
  value,
  label,
  className,
  align = "center"
}) {
  return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0,
    "div",
    {
      className: _chunkMD6ORKN4cjs.cn.call(void 0,
        "flex flex-col gap-1",
        align === "center" ? "items-center text-center" : "items-start",
        className
      ),
      children: [
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "text-2xl font-bold tabular-nums text-foreground", children: value }),
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "text-sm text-muted-foreground", children: label })
      ]
    }
  );
}
function StatPair({
  left,
  right,
  className
}) {
  return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: _chunkMD6ORKN4cjs.cn.call(void 0, "grid grid-cols-2", className), children: [
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0, Stat, { value: left.value, label: left.label }),
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0, Stat, { value: right.value, label: right.label, className: "border-l border-border" })
  ] });
}




exports.Stat = Stat; exports.StatPair = StatPair;
//# sourceMappingURL=chunk-PR6YX3JC.cjs.map