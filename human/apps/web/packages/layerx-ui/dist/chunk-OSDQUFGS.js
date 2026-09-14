"use client";
import {
  cn
} from "./chunk-LXFZWLUU.js";

// src/components/code-input.tsx
import * as React from "react";
import { Delete } from "lucide-react";
import { jsx, jsxs } from "react/jsx-runtime";
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
      inputRef.current?.focus({ preventScroll: true });
    }
  }, []);
  const commit = (next) => {
    const clean = next.replace(/\D/g, "").slice(0, length);
    onChange(clean);
    if (clean.length === length) onComplete?.(clean);
  };
  const activeIndex = Math.min(value.length, length - 1);
  return /* @__PURE__ */ jsxs(
    "div",
    {
      className: cn("relative", className),
      onClick: () => !readOnly && inputRef.current?.focus(),
      children: [
        /* @__PURE__ */ jsx(
          "input",
          {
            ref: inputRef,
            type: "text",
            inputMode: "numeric",
            autoComplete: "one-time-code",
            "aria-invalid": error || void 0,
            "aria-label": aria["aria-label"] ?? "Verification code",
            className: cn(
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
        /* @__PURE__ */ jsx("div", { className: "flex items-center justify-center gap-2.5", "aria-hidden": true, children: Array.from({ length }).map((_, i) => {
          const char = value[i];
          const isActive = focused && !disabled && i === activeIndex;
          return /* @__PURE__ */ jsx(
            "span",
            {
              className: cn(
                "flex size-12 items-center justify-center rounded-md border bg-surface text-xl font-semibold tabular-nums transition-colors",
                error ? "border-destructive text-destructive" : isActive ? "border-accent ring-2 ring-accent/20 text-foreground" : "border-border text-foreground",
                disabled && "opacity-50"
              ),
              children: char ?? ""
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
  return /* @__PURE__ */ jsx("div", { className: cn("grid grid-cols-3 gap-px overflow-hidden rounded-lg bg-border", className), children: keys.map(
    (k) => k.main === "back" ? /* @__PURE__ */ jsx(
      "button",
      {
        type: "button",
        "aria-label": "Backspace",
        onClick: onBackspace,
        className: "flex h-14 items-center justify-center bg-surface text-foreground transition-colors active:bg-surface-sunken",
        children: /* @__PURE__ */ jsx(Delete, { className: "size-5" })
      },
      "back"
    ) : /* @__PURE__ */ jsxs(
      "button",
      {
        type: "button",
        onClick: () => /^\d$/.test(k.main) && onDigit(k.main),
        className: "flex h-14 flex-col items-center justify-center gap-0 bg-surface text-foreground transition-colors active:bg-surface-sunken",
        children: [
          /* @__PURE__ */ jsx("span", { className: "text-xl font-semibold leading-tight", children: k.main }),
          k.sub && /* @__PURE__ */ jsx("span", { className: "text-[9px] font-semibold tracking-[0.18em] text-muted-foreground", children: k.sub })
        ]
      },
      k.main
    )
  ) });
}

export {
  CodeInput,
  Keypad
};
//# sourceMappingURL=chunk-OSDQUFGS.js.map