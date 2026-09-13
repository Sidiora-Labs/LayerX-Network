"use strict";Object.defineProperty(exports, "__esModule", {value: true});"use client";


var _chunkMD6ORKN4cjs = require('./chunk-MD6ORKN4.cjs');

// src/components/empty-state.tsx
var _jsxruntime = require('react/jsx-runtime');
function EmptyState({
  icon,
  title,
  description,
  action,
  className
}) {
  return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, 
    "div",
    {
      className: _chunkMD6ORKN4cjs.cn.call(void 0, 
        "flex flex-col items-center gap-2.5 rounded-lg bg-surface px-6 py-10 text-center",
        className
      ),
      children: [
        icon && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "mb-1 inline-flex size-16 items-center justify-center rounded-full bg-surface-sunken text-muted-foreground [&_svg]:size-7", children: icon }),
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "h3", { className: "text-[17px] font-bold text-foreground", children: title }),
        description && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "p", { className: "max-w-[280px] text-sm leading-relaxed text-muted-foreground", children: description }),
        action && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { className: "mt-3", children: action })
      ]
    }
  );
}



exports.EmptyState = EmptyState;
//# sourceMappingURL=chunk-MEQICWTK.cjs.map