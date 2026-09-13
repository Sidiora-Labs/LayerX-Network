"use client";
import {
  PrimaryAction
} from "./chunk-HMBLSESK.js";
import {
  usePlatform
} from "./chunk-XORHQGZG.js";
import {
  Button,
  IconButton
} from "./chunk-4X2DK7Y3.js";
import {
  cn
} from "./chunk-LXFZWLUU.js";

// src/components/wizard.tsx
import * as React from "react";
import { ArrowLeft, Check } from "lucide-react";
import { jsx, jsxs } from "react/jsx-runtime";
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
  const resolved = usePlatform(platform);
  const [index, setIndex] = React.useState(0);
  const step = steps[index];
  const isLast = index === steps.length - 1;
  const canContinue = step.canContinue ? step.canContinue() : true;
  const next = () => {
    if (isLast) onComplete?.();
    else setIndex((i) => Math.min(i + 1, steps.length - 1));
  };
  const back = () => {
    if (index === 0) onCancel?.();
    else setIndex((i) => Math.max(0, i - 1));
  };
  const stepBody = /* @__PURE__ */ jsxs("div", { className: "flex flex-col gap-2", children: [
    /* @__PURE__ */ jsx("h2", { className: "text-xl font-bold text-foreground", children: step.title }),
    step.description && /* @__PURE__ */ jsx("p", { className: "text-[15px] leading-relaxed text-muted-foreground", children: step.description }),
    /* @__PURE__ */ jsx("div", { className: "pt-4", children: step.render() })
  ] });
  if (resolved === "mobile") {
    return /* @__PURE__ */ jsxs("div", { className: cn("flex h-full min-h-0 flex-1 flex-col", className), children: [
      /* @__PURE__ */ jsxs("div", { className: "flex items-center gap-3 px-4 pt-2 pb-4", children: [
        /* @__PURE__ */ jsx(IconButton, { variant: "outline", size: "sm", onClick: back, "aria-label": "Back", children: /* @__PURE__ */ jsx(ArrowLeft, {}) }),
        /* @__PURE__ */ jsx("div", { className: "flex flex-1 items-center gap-1.5", "aria-hidden": true, children: steps.map((s, i) => /* @__PURE__ */ jsx(
          "span",
          {
            className: cn(
              "h-1 flex-1 rounded-full transition-colors",
              i <= index ? "bg-foreground" : "bg-border"
            )
          },
          s.id
        )) }),
        /* @__PURE__ */ jsxs("span", { className: "text-xs font-semibold text-muted-foreground tabular-nums", children: [
          index + 1,
          "/",
          steps.length
        ] })
      ] }),
      /* @__PURE__ */ jsxs("div", { className: "lx-scroll flex min-h-0 flex-1 flex-col overflow-y-auto px-4", children: [
        stepBody,
        /* @__PURE__ */ jsx(PrimaryAction, { onClick: next, disabled: !canContinue, platform: "mobile", children: isLast ? completeLabel : "Continue" })
      ] })
    ] });
  }
  return /* @__PURE__ */ jsxs("div", { className: cn("grid min-h-0 flex-1 grid-cols-[1fr_340px] gap-8", className), children: [
    /* @__PURE__ */ jsxs("div", { className: "flex min-h-0 flex-col", children: [
      /* @__PURE__ */ jsx("ol", { className: "flex items-center gap-2 pb-6", "aria-label": "Progress", children: steps.map((s, i) => /* @__PURE__ */ jsxs("li", { className: "flex items-center gap-2", children: [
        /* @__PURE__ */ jsx(
          "span",
          {
            className: cn(
              "inline-flex size-6 items-center justify-center rounded-full text-xs font-bold",
              i < index ? "bg-success text-success-foreground" : i === index ? "bg-primary text-primary-foreground" : "bg-surface-sunken text-faint-foreground"
            ),
            children: i < index ? /* @__PURE__ */ jsx(Check, { className: "size-3.5" }) : i + 1
          }
        ),
        /* @__PURE__ */ jsx(
          "span",
          {
            className: cn(
              "text-sm font-semibold",
              i === index ? "text-foreground" : "text-muted-foreground"
            ),
            children: s.label
          }
        ),
        i < steps.length - 1 && /* @__PURE__ */ jsx("span", { className: "mx-1 h-px w-6 bg-border", "aria-hidden": true })
      ] }, s.id)) }),
      /* @__PURE__ */ jsx("div", { className: "lx-scroll min-h-0 flex-1 overflow-y-auto pr-2", children: stepBody }),
      /* @__PURE__ */ jsxs("div", { className: "flex items-center gap-3 border-t border-border pt-4", children: [
        /* @__PURE__ */ jsx(Button, { variant: "secondary", onClick: back, children: index === 0 ? "Cancel" : "Back" }),
        /* @__PURE__ */ jsx(Button, { onClick: next, disabled: !canContinue, className: "min-w-[160px]", children: isLast ? completeLabel : "Continue" })
      ] })
    ] }),
    /* @__PURE__ */ jsxs("aside", { className: "sticky top-0 h-fit rounded-lg border border-border bg-surface p-5 shadow-card", children: [
      /* @__PURE__ */ jsx("h3", { className: "text-sm font-bold tracking-wide text-muted-foreground uppercase", children: summaryTitle }),
      /* @__PURE__ */ jsxs("dl", { className: "mt-3 flex flex-col divide-y divide-border/70", children: [
        (summary ?? []).map((item) => /* @__PURE__ */ jsxs("div", { className: "flex items-center justify-between gap-4 py-3", children: [
          /* @__PURE__ */ jsx("dt", { className: "text-sm text-muted-foreground", children: item.label }),
          /* @__PURE__ */ jsx("dd", { className: "text-right text-sm font-semibold text-foreground", children: item.value })
        ] }, item.label)),
        (!summary || summary.length === 0) && /* @__PURE__ */ jsx("p", { className: "py-3 text-sm text-faint-foreground", children: "Your choices will appear here as you go." })
      ] })
    ] })
  ] });
}

export {
  Wizard
};
//# sourceMappingURL=chunk-QMRBFKQK.js.map