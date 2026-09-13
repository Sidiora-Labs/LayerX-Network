"use client";
import {
  cn
} from "./chunk-LXFZWLUU.js";

// src/components/empty-state.tsx
import { jsx, jsxs } from "react/jsx-runtime";
function EmptyState({
  icon,
  title,
  description,
  action,
  className
}) {
  return /* @__PURE__ */ jsxs(
    "div",
    {
      className: cn(
        "flex flex-col items-center gap-2.5 rounded-lg bg-surface px-6 py-10 text-center",
        className
      ),
      children: [
        icon && /* @__PURE__ */ jsx("span", { className: "mb-1 inline-flex size-16 items-center justify-center rounded-full bg-surface-sunken text-muted-foreground [&_svg]:size-7", children: icon }),
        /* @__PURE__ */ jsx("h3", { className: "text-[17px] font-bold text-foreground", children: title }),
        description && /* @__PURE__ */ jsx("p", { className: "max-w-[280px] text-sm leading-relaxed text-muted-foreground", children: description }),
        action && /* @__PURE__ */ jsx("div", { className: "mt-3", children: action })
      ]
    }
  );
}

export {
  EmptyState
};
//# sourceMappingURL=chunk-34BAVXSZ.js.map