"use strict";Object.defineProperty(exports, "__esModule", {value: true}); function _optionalChain(ops) { let lastAccessLHS = undefined; let value = ops[0]; let i = 1; while (i < ops.length) { const op = ops[i]; const fn = ops[i + 1]; i += 2; if ((op === 'optionalAccess' || op === 'optionalCall') && value == null) { return undefined; } if (op === 'access' || op === 'optionalAccess') { lastAccessLHS = value; value = fn(value); } else if (op === 'call' || op === 'optionalCall') { value = fn((...args) => value.call(lastAccessLHS, ...args)); lastAccessLHS = undefined; } } return value; }"use client";


var _chunkMD6ORKN4cjs = require('./chunk-MD6ORKN4.cjs');

// src/components/quick-actions.tsx
var _jsxruntime = require('react/jsx-runtime');
function QuickActions({
  actions,
  onAction,
  className
}) {
  return /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
    "div",
    {
      className: _chunkMD6ORKN4cjs.cn.call(void 0, "grid gap-2", className),
      style: { gridTemplateColumns: `repeat(${Math.min(actions.length, 5)}, minmax(0, 1fr))` },
      children: actions.map((a) => /* @__PURE__ */ _jsxruntime.jsxs.call(void 0,
        "button",
        {
          type: "button",
          onClick: () => _optionalChain([onAction, 'optionalCall', _ => _(a.id)]),
          className: "group flex flex-col items-center gap-2 outline-none",
          children: [
            /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "inline-flex size-12 items-center justify-center rounded-full border border-border bg-surface text-foreground transition-colors group-hover:bg-surface-sunken group-focus-visible:ring-2 group-focus-visible:ring-accent/30 [&_svg]:size-5", children: a.icon }),
            /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "text-[13px] font-medium text-foreground-secondary", children: a.label })
          ]
        },
        a.id
      ))
    }
  );
}



exports.QuickActions = QuickActions;
//# sourceMappingURL=chunk-OSIIJJQL.cjs.map