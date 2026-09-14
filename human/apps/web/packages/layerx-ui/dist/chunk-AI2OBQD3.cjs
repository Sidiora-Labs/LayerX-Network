"use strict";Object.defineProperty(exports, "__esModule", {value: true});"use client";


var _chunkMD6ORKN4cjs = require('./chunk-MD6ORKN4.cjs');

// src/components/feedback.tsx
var _lucidereact = require('lucide-react');
var _jsxruntime = require('react/jsx-runtime');
function Spinner({ className }) {
  return /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.Loader2, { className: _chunkMD6ORKN4cjs.cn.call(void 0, "size-5 animate-spin text-muted-foreground", className), "aria-label": "Loading" });
}
function Skeleton({ className }) {
  return /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { className: _chunkMD6ORKN4cjs.cn.call(void 0, "animate-pulse rounded-md bg-surface-sunken", className) });
}
function SkeletonRow() {
  return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "flex items-center gap-3 py-3.5", children: [
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0, Skeleton, { className: "size-11 rounded-full" }),
    /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "flex flex-1 flex-col gap-2", children: [
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, Skeleton, { className: "h-3.5 w-1/3" }),
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, Skeleton, { className: "h-3 w-1/4" })
    ] }),
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0, Skeleton, { className: "h-3.5 w-16" })
  ] });
}





exports.Spinner = Spinner; exports.Skeleton = Skeleton; exports.SkeletonRow = SkeletonRow;
//# sourceMappingURL=chunk-AI2OBQD3.cjs.map