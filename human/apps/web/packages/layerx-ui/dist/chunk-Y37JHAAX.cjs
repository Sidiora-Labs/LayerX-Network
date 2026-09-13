"use strict";Object.defineProperty(exports, "__esModule", {value: true}); function _interopRequireWildcard(obj) { if (obj && obj.__esModule) { return obj; } else { var newObj = {}; if (obj != null) { for (var key in obj) { if (Object.prototype.hasOwnProperty.call(obj, key)) { newObj[key] = obj[key]; } } } newObj.default = obj; return newObj; } } function _nullishCoalesce(lhs, rhsFn) { if (lhs != null) { return lhs; } else { return rhsFn(); } } function _optionalChain(ops) { let lastAccessLHS = undefined; let value = ops[0]; let i = 1; while (i < ops.length) { const op = ops[i]; const fn = ops[i + 1]; i += 2; if ((op === 'optionalAccess' || op === 'optionalCall') && value == null) { return undefined; } if (op === 'access' || op === 'optionalAccess') { lastAccessLHS = value; value = fn(value); } else if (op === 'call' || op === 'optionalCall') { value = fn((...args) => value.call(lastAccessLHS, ...args)); lastAccessLHS = undefined; } } return value; }"use client";


var _chunkFWX3RTNDcjs = require('./chunk-FWX3RTND.cjs');


var _chunkI62LU2PGcjs = require('./chunk-I62LU2PG.cjs');



var _chunkRA3A4XJ2cjs = require('./chunk-RA3A4XJ2.cjs');


var _chunkMD6ORKN4cjs = require('./chunk-MD6ORKN4.cjs');

// src/components/wizard.tsx
var _react = require('react'); var React = _interopRequireWildcard(_react);
var _lucidereact = require('lucide-react');
var _jsxruntime = require('react/jsx-runtime');
function Wizard({
  steps,
  summary,
  onComplete,
  onCancel,
  completeLabel = "Confirm",
  summaryTitle = "What will happen",
  platform,
  className
}) {
  const resolved = _chunkI62LU2PGcjs.usePlatform.call(void 0, platform);
  const [index, setIndex] = React.useState(0);
  const step = steps[index];
  const isLast = index === steps.length - 1;
  const canContinue = step.canContinue ? step.canContinue() : true;
  const next = () => {
    if (isLast) _optionalChain([onComplete, 'optionalCall', _ => _()]);
    else setIndex((i) => Math.min(i + 1, steps.length - 1));
  };
  const back = () => {
    if (index === 0) _optionalChain([onCancel, 'optionalCall', _2 => _2()]);
    else setIndex((i) => Math.max(0, i - 1));
  };
  const stepBody = /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "flex flex-col gap-2", children: [
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "h2", { className: "text-xl font-bold text-foreground", children: step.title }),
    step.description && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "p", { className: "text-[15px] leading-relaxed text-muted-foreground", children: step.description }),
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { className: "pt-4", children: step.render() })
  ] });
  if (resolved === "mobile") {
    return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: _chunkMD6ORKN4cjs.cn.call(void 0, "flex h-full min-h-0 flex-1 flex-col", className), children: [
      /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "flex items-center gap-3 px-4 pt-2 pb-4", children: [
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkRA3A4XJ2cjs.IconButton, { variant: "outline", size: "sm", onClick: back, "aria-label": "Back", children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.ArrowLeft, {}) }),
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { className: "flex flex-1 items-center gap-1.5", "aria-hidden": true, children: steps.map((s, i) => /* @__PURE__ */ _jsxruntime.jsx.call(void 0, 
          "span",
          {
            className: _chunkMD6ORKN4cjs.cn.call(void 0, 
              "h-1 flex-1 rounded-full transition-colors",
              i <= index ? "bg-foreground" : "bg-border"
            )
          },
          s.id
        )) }),
        /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "span", { className: "text-xs font-semibold text-muted-foreground tabular-nums", children: [
          index + 1,
          "/",
          steps.length
        ] })
      ] }),
      /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "lx-scroll flex min-h-0 flex-1 flex-col overflow-y-auto px-4", children: [
        stepBody,
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkFWX3RTNDcjs.PrimaryAction, { onClick: next, disabled: !canContinue, platform: "mobile", children: isLast ? completeLabel : "Continue" })
      ] })
    ] });
  }
  return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: _chunkMD6ORKN4cjs.cn.call(void 0, "grid min-h-0 flex-1 grid-cols-[1fr_340px] gap-8", className), children: [
    /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "flex min-h-0 flex-col", children: [
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "ol", { className: "flex items-center gap-2 pb-6", "aria-label": "Progress", children: steps.map((s, i) => /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "li", { className: "flex items-center gap-2", children: [
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, 
          "span",
          {
            className: _chunkMD6ORKN4cjs.cn.call(void 0, 
              "inline-flex size-6 items-center justify-center rounded-full text-xs font-bold",
              i < index ? "bg-success text-success-foreground" : i === index ? "bg-primary text-primary-foreground" : "bg-surface-sunken text-faint-foreground"
            ),
            children: i < index ? /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.Check, { className: "size-3.5" }) : i + 1
          }
        ),
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, 
          "span",
          {
            className: _chunkMD6ORKN4cjs.cn.call(void 0, 
              "text-sm font-semibold",
              i === index ? "text-foreground" : "text-muted-foreground"
            ),
            children: s.label
          }
        ),
        i < steps.length - 1 && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "mx-1 h-px w-6 bg-border", "aria-hidden": true })
      ] }, s.id)) }),
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { className: "lx-scroll min-h-0 flex-1 overflow-y-auto pr-2", children: stepBody }),
      /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "flex items-center gap-3 border-t border-border pt-4", children: [
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkRA3A4XJ2cjs.Button, { variant: "secondary", onClick: back, children: index === 0 ? "Cancel" : "Back" }),
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkRA3A4XJ2cjs.Button, { onClick: next, disabled: !canContinue, className: "min-w-[160px]", children: isLast ? completeLabel : "Continue" })
      ] })
    ] }),
    /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "aside", { className: "sticky top-0 h-fit rounded-lg border border-border bg-surface p-5 shadow-card", children: [
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "h3", { className: "text-sm font-bold tracking-wide text-muted-foreground uppercase", children: summaryTitle }),
      /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "dl", { className: "mt-3 flex flex-col divide-y divide-border/70", children: [
        (_nullishCoalesce(summary, () => ( []))).map((item) => /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "flex items-center justify-between gap-4 py-3", children: [
          /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "dt", { className: "text-sm text-muted-foreground", children: item.label }),
          /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "dd", { className: "text-right text-sm font-semibold text-foreground", children: item.value })
        ] }, item.label)),
        (!summary || summary.length === 0) && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "p", { className: "py-3 text-sm text-faint-foreground", children: "Your choices will appear here as you go." })
      ] })
    ] })
  ] });
}



exports.Wizard = Wizard;
//# sourceMappingURL=chunk-Y37JHAAX.cjs.map