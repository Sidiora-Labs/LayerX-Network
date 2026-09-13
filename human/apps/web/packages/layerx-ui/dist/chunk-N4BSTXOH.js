"use client";
import {
  CodeInput,
  Keypad
} from "./chunk-OSDQUFGS.js";
import {
  usePlatform
} from "./chunk-XORHQGZG.js";
import {
  cn
} from "./chunk-LXFZWLUU.js";

// src/components/code-entry.tsx
import { Fragment, jsx, jsxs } from "react/jsx-runtime";
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
  const resolved = usePlatform(platform);
  const mobile = resolved === "mobile";
  return /* @__PURE__ */ jsxs("div", { className: cn("flex flex-col gap-4", className), children: [
    /* @__PURE__ */ jsx(
      CodeInput,
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
    error && errorText && /* @__PURE__ */ jsx("p", { role: "alert", className: "text-center text-[13px] font-medium text-destructive", children: errorText }),
    (onResend || (resendIn ?? 0) > 0) && /* @__PURE__ */ jsx("p", { className: "text-center text-sm text-muted-foreground", children: (resendIn ?? 0) > 0 ? /* @__PURE__ */ jsxs(Fragment, { children: [
      "Resend in",
      " ",
      /* @__PURE__ */ jsxs("span", { className: "font-semibold tabular-nums text-accent-strong", children: [
        "00:",
        String(resendIn).padStart(2, "0")
      ] })
    ] }) : /* @__PURE__ */ jsx(
      "button",
      {
        type: "button",
        onClick: onResend,
        className: "font-semibold text-accent hover:underline",
        children: "Resend code"
      }
    ) }),
    mobile && /* @__PURE__ */ jsx(
      Keypad,
      {
        className: "mt-2",
        onDigit: (d) => {
          if (value.length < length) {
            const next = value + d;
            onChange(next);
            if (next.length === length) onComplete?.(next);
          }
        },
        onBackspace: () => onChange(value.slice(0, -1))
      }
    )
  ] });
}

export {
  CodeEntry
};
//# sourceMappingURL=chunk-N4BSTXOH.js.map