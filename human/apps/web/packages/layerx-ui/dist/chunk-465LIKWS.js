"use client";
import {
  cn
} from "./chunk-LXFZWLUU.js";

// src/components/feedback.tsx
import { Loader2 } from "lucide-react";
import { jsx, jsxs } from "react/jsx-runtime";
function Spinner({ className }) {
  return /* @__PURE__ */ jsx(Loader2, { className: cn("size-5 animate-spin text-muted-foreground", className), "aria-label": "Loading" });
}
function Skeleton({ className }) {
  return /* @__PURE__ */ jsx("div", { className: cn("animate-pulse rounded-md bg-surface-sunken", className) });
}
function SkeletonRow() {
  return /* @__PURE__ */ jsxs("div", { className: "flex items-center gap-3 py-3.5", children: [
    /* @__PURE__ */ jsx(Skeleton, { className: "size-11 rounded-full" }),
    /* @__PURE__ */ jsxs("div", { className: "flex flex-1 flex-col gap-2", children: [
      /* @__PURE__ */ jsx(Skeleton, { className: "h-3.5 w-1/3" }),
      /* @__PURE__ */ jsx(Skeleton, { className: "h-3 w-1/4" })
    ] }),
    /* @__PURE__ */ jsx(Skeleton, { className: "h-3.5 w-16" })
  ] });
}

export {
  Spinner,
  Skeleton,
  SkeletonRow
};
//# sourceMappingURL=chunk-465LIKWS.js.map