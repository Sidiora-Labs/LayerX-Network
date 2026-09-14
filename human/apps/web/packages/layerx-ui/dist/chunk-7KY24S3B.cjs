"use strict";Object.defineProperty(exports, "__esModule", {value: true}); function _nullishCoalesce(lhs, rhsFn) { if (lhs != null) { return lhs; } else { return rhsFn(); } } function _optionalChain(ops) { let lastAccessLHS = undefined; let value = ops[0]; let i = 1; while (i < ops.length) { const op = ops[i]; const fn = ops[i + 1]; i += 2; if ((op === 'optionalAccess' || op === 'optionalCall') && value == null) { return undefined; } if (op === 'access' || op === 'optionalAccess') { lastAccessLHS = value; value = fn(value); } else if (op === 'call' || op === 'optionalCall') { value = fn((...args) => value.call(lastAccessLHS, ...args)); lastAccessLHS = undefined; } } return value; }"use client";



var _chunkMOEIKVUAcjs = require('./chunk-MOEIKVUA.cjs');


var _chunkI62LU2PGcjs = require('./chunk-I62LU2PG.cjs');


var _chunkMD6ORKN4cjs = require('./chunk-MD6ORKN4.cjs');

// src/components/code-entry.tsx
var _jsxruntime = require('react/jsx-runtime');
function CodeEntry({
  length = 6,
  value,
  onChange,
  onComplete,
  error,
  errorText,
  resendIn,
  onResend,
  platform,
  className
}) {
  const resolved = _chunkI62LU2PGcjs.usePlatform.call(void 0, platform);
  const mobile = resolved === "mobile";
  return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: _chunkMD6ORKN4cjs.cn.call(void 0, "flex flex-col gap-4", className), children: [
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
      _chunkMOEIKVUAcjs.CodeInput,
      {
        length,
        value,
        onChange,
        onComplete,
        error,
        readOnly: mobile,
        autoFocus: !mobile,
        "aria-label": "Verification code"
      }
    ),
    error && errorText && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "p", { role: "alert", className: "text-center text-[13px] font-medium text-destructive", children: errorText }),
    (onResend || (_nullishCoalesce(resendIn, () => ( 0))) > 0) && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "p", { className: "text-center text-sm text-muted-foreground", children: (_nullishCoalesce(resendIn, () => ( 0))) > 0 ? /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, _jsxruntime.Fragment, { children: [
      "Resend in",
      " ",
      /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "span", { className: "font-semibold tabular-nums text-accent-strong", children: [
        "00:",
        String(resendIn).padStart(2, "0")
      ] })
    ] }) : /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
      "button",
      {
        type: "button",
        onClick: onResend,
        className: "font-semibold text-accent hover:underline",
        children: "Resend code"
      }
    ) }),
    mobile && /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
      _chunkMOEIKVUAcjs.Keypad,
      {
        className: "mt-2",
        onDigit: (d) => {
          if (value.length < length) {
            const next = value + d;
            onChange(next);
            if (next.length === length) _optionalChain([onComplete, 'optionalCall', _ => _(next)]);
          }
        },
        onBackspace: () => onChange(value.slice(0, -1))
      }
    )
  ] });
}



exports.CodeEntry = CodeEntry;
//# sourceMappingURL=chunk-7KY24S3B.cjs.map