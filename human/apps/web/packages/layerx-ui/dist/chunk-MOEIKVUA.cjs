"use strict";Object.defineProperty(exports, "__esModule", {value: true}); function _interopRequireWildcard(obj) { if (obj && obj.__esModule) { return obj; } else { var newObj = {}; if (obj != null) { for (var key in obj) { if (Object.prototype.hasOwnProperty.call(obj, key)) { newObj[key] = obj[key]; } } } newObj.default = obj; return newObj; } } function _nullishCoalesce(lhs, rhsFn) { if (lhs != null) { return lhs; } else { return rhsFn(); } } function _optionalChain(ops) { let lastAccessLHS = undefined; let value = ops[0]; let i = 1; while (i < ops.length) { const op = ops[i]; const fn = ops[i + 1]; i += 2; if ((op === 'optionalAccess' || op === 'optionalCall') && value == null) { return undefined; } if (op === 'access' || op === 'optionalAccess') { lastAccessLHS = value; value = fn(value); } else if (op === 'call' || op === 'optionalCall') { value = fn((...args) => value.call(lastAccessLHS, ...args)); lastAccessLHS = undefined; } } return value; }"use client";


var _chunkMD6ORKN4cjs = require('./chunk-MD6ORKN4.cjs');

// src/components/code-input.tsx
var _react = require('react'); var React = _interopRequireWildcard(_react);
var _lucidereact = require('lucide-react');
var _jsxruntime = require('react/jsx-runtime');
function CodeInput({
  length = 6,
  value,
  onChange,
  onComplete,
  error,
  disabled,
  autoFocus,
  readOnly,
  className,
  ...aria
}) {
  const inputRef = React.useRef(null);
  const [focused, setFocused] = React.useState(false);
  React.useEffect(() => {
    if (autoFocus && !readOnly && !disabled) {
      _optionalChain([inputRef, 'access', _2 => _2.current, 'optionalAccess', _3 => _3.focus, 'call', _4 => _4({ preventScroll: true })]);
    }
  }, []);
  const commit = (next) => {
    const clean = next.replace(/\D/g, "").slice(0, length);
    onChange(clean);
    if (clean.length === length) _optionalChain([onComplete, 'optionalCall', _5 => _5(clean)]);
  };
  const activeIndex = Math.min(value.length, length - 1);
  return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0,
    "div",
    {
      className: _chunkMD6ORKN4cjs.cn.call(void 0, "relative", className),
      onClick: () => !readOnly && _optionalChain([inputRef, 'access', _6 => _6.current, 'optionalAccess', _7 => _7.focus, 'call', _8 => _8()]),
      children: [
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
          "input",
          {
            ref: inputRef,
            type: "text",
            inputMode: "numeric",
            autoComplete: "one-time-code",
            "aria-invalid": error || void 0,
            "aria-label": _nullishCoalesce(aria["aria-label"], () => ( "Verification code")),
            className: _chunkMD6ORKN4cjs.cn.call(void 0,
              "absolute inset-0 h-full w-full opacity-0",
              readOnly ? "pointer-events-none" : "cursor-text"
            ),
            value,
            disabled,
            readOnly,
            onFocus: () => setFocused(true),
            onBlur: () => setFocused(false),
            onChange: (e) => commit(e.target.value),
            onPaste: (e) => {
              e.preventDefault();
              commit(e.clipboardData.getData("text"));
            }
          }
        ),
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { className: "flex items-center justify-center gap-2.5", "aria-hidden": true, children: Array.from({ length }).map((_, i) => {
          const char = value[i];
          const isActive = focused && !disabled && i === activeIndex;
          return /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
            "span",
            {
              className: _chunkMD6ORKN4cjs.cn.call(void 0,
                "flex size-12 items-center justify-center rounded-md border bg-surface text-xl font-semibold tabular-nums transition-colors",
                error ? "border-destructive text-destructive" : isActive ? "border-accent ring-2 ring-accent/20 text-foreground" : "border-border text-foreground",
                disabled && "opacity-50"
              ),
              children: _nullishCoalesce(char, () => ( ""))
            },
            i
          );
        }) })
      ]
    }
  );
}
function Keypad({
  onDigit,
  onBackspace,
  className
}) {
  const keys = [
    { main: "1" },
    { main: "2", sub: "ABC" },
    { main: "3", sub: "DEF" },
    { main: "4", sub: "GHI" },
    { main: "5", sub: "JKL" },
    { main: "6", sub: "MNO" },
    { main: "7", sub: "PQRS" },
    { main: "8", sub: "TUV" },
    { main: "9", sub: "WXYZ" },
    { main: "+*#" },
    { main: "0" },
    { main: "back" }
  ];
  return /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { className: _chunkMD6ORKN4cjs.cn.call(void 0, "grid grid-cols-3 gap-px overflow-hidden rounded-lg bg-border", className), children: keys.map(
    (k) => k.main === "back" ? /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
      "button",
      {
        type: "button",
        "aria-label": "Backspace",
        onClick: onBackspace,
        className: "flex h-14 items-center justify-center bg-surface text-foreground transition-colors active:bg-surface-sunken",
        children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.Delete, { className: "size-5" })
      },
      "back"
    ) : /* @__PURE__ */ _jsxruntime.jsxs.call(void 0,
      "button",
      {
        type: "button",
        onClick: () => /^\d$/.test(k.main) && onDigit(k.main),
        className: "flex h-14 flex-col items-center justify-center gap-0 bg-surface text-foreground transition-colors active:bg-surface-sunken",
        children: [
          /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "text-xl font-semibold leading-tight", children: k.main }),
          k.sub && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "text-[9px] font-semibold tracking-[0.18em] text-muted-foreground", children: k.sub })
        ]
      },
      k.main
    )
  ) });
}




exports.CodeInput = CodeInput; exports.Keypad = Keypad;
//# sourceMappingURL=chunk-MOEIKVUA.cjs.map