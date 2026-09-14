"use strict";Object.defineProperty(exports, "__esModule", {value: true}); function _interopRequireWildcard(obj) { if (obj && obj.__esModule) { return obj; } else { var newObj = {}; if (obj != null) { for (var key in obj) { if (Object.prototype.hasOwnProperty.call(obj, key)) { newObj[key] = obj[key]; } } } newObj.default = obj; return newObj; } } function _nullishCoalesce(lhs, rhsFn) { if (lhs != null) { return lhs; } else { return rhsFn(); } } function _optionalChain(ops) { let lastAccessLHS = undefined; let value = ops[0]; let i = 1; while (i < ops.length) { const op = ops[i]; const fn = ops[i + 1]; i += 2; if ((op === 'optionalAccess' || op === 'optionalCall') && value == null) { return undefined; } if (op === 'access' || op === 'optionalAccess') { lastAccessLHS = value; value = fn(value); } else if (op === 'call' || op === 'optionalCall') { value = fn((...args) => value.call(lastAccessLHS, ...args)); lastAccessLHS = undefined; } } return value; }"use client";


var _chunkW6TE4RURcjs = require('./chunk-W6TE4RUR.cjs');


var _chunkMD6ORKN4cjs = require('./chunk-MD6ORKN4.cjs');

// src/components/balance-header.tsx
var _react = require('react'); var React = _interopRequireWildcard(_react);
var _lucidereact = require('lucide-react');
var _jsxruntime = require('react/jsx-runtime');
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
  const hidden = _nullishCoalesce(hiddenProp, () => ( internalHidden));
  const toggle = () => {
    const next = !hidden;
    setInternalHidden(next);
    _optionalChain([onHiddenChange, 'optionalCall', _ => _(next)]);
  };
  return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: _chunkMD6ORKN4cjs.cn.call(void 0, "flex flex-col gap-1", align === "center" && "items-center", className), children: [
    label && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "text-sm text-muted-foreground", children: label }),
    /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "flex items-center gap-2.5", children: [
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "text-[32px] leading-none font-extrabold tabular-nums tracking-tight text-foreground", children: hidden ? `${symbol} \u2022\u2022\u2022\u2022\u2022\u2022` : _chunkW6TE4RURcjs.formatBalance.call(void 0, value, symbol) }),
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
        "button",
        {
          type: "button",
          onClick: toggle,
          "aria-label": hidden ? "Show balance" : "Hide balance",
          className: "inline-flex size-7 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-surface-sunken",
          children: hidden ? /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.EyeOff, { className: "size-[18px]" }) : /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.Eye, { className: "size-[18px]" })
        }
      )
    ] }),
    change && !hidden && /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "span", { className: "flex items-center gap-1.5 text-sm text-muted-foreground", children: [
      "1 day change:",
      /* @__PURE__ */ _jsxruntime.jsxs.call(void 0,
        "span",
        {
          className: _chunkMD6ORKN4cjs.cn.call(void 0,
            "flex items-center gap-1 font-semibold",
            change.up ? "text-success" : "text-destructive"
          ),
          children: [
            change.text,
            change.up ? /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.TrendingUp, { className: "size-4", "aria-hidden": true }) : /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.TrendingDown, { className: "size-4", "aria-hidden": true })
          ]
        }
      )
    ] })
  ] });
}



exports.BalanceHeader = BalanceHeader;
//# sourceMappingURL=chunk-IFM4RXKS.cjs.map