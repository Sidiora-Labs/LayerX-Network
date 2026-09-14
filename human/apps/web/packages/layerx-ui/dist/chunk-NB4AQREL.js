"use client";
import {
  Sheet,
  SheetBody,
  SheetHeader
} from "./chunk-MJDZCIQM.js";
import {
  Drawer,
  DrawerBody,
  DrawerHeader
} from "./chunk-QZV34S35.js";
import {
  usePlatform
} from "./chunk-XORHQGZG.js";
import {
  IconButton
} from "./chunk-4X2DK7Y3.js";
import {
  cn
} from "./chunk-LXFZWLUU.js";

// src/components/detail.tsx
import * as React from "react";
import { ArrowLeft, ChevronDown } from "lucide-react";
import { jsx, jsxs } from "react/jsx-runtime";
function DetailDisclosure({
  open,
  onOpenChange,
  title,
  children,
  mobileVariant = "sheet",
  desktopVariant = "drawer",
  platform,
  portalContainer,
  summary
}) {
  const resolved = usePlatform(platform);
  const disclosureId = React.useId();
  if (resolved === "desktop" && desktopVariant === "inline") {
    return /* @__PURE__ */ jsxs("div", { className: "overflow-hidden rounded-lg border border-border bg-surface", children: [
      /* @__PURE__ */ jsxs(
        "button",
        {
          type: "button",
          "aria-expanded": open,
          "aria-controls": disclosureId,
          onClick: () => onOpenChange(!open),
          className: "flex w-full items-center justify-between gap-3 px-5 py-4 text-left font-semibold text-foreground transition-colors hover:bg-surface-sunken/40",
          children: [
            /* @__PURE__ */ jsx("span", { children: summary ?? title }),
            /* @__PURE__ */ jsx(
              ChevronDown,
              {
                className: cn("size-4 text-muted-foreground transition-transform", open && "rotate-180")
              }
            )
          ]
        }
      ),
      /* @__PURE__ */ jsx(
        "div",
        {
          id: disclosureId,
          role: "region",
          className: cn(
            "grid transition-[grid-template-rows] duration-300",
            open ? "grid-rows-[1fr]" : "grid-rows-[0fr]"
          ),
          children: /* @__PURE__ */ jsx("div", { className: "overflow-hidden", children: /* @__PURE__ */ jsx("div", { className: "border-t border-border px-5 py-4", children }) })
        }
      )
    ] });
  }
  if (resolved === "mobile" && mobileVariant === "pushed") {
    if (!open) return null;
    return /* @__PURE__ */ jsxs("div", { className: "fixed inset-0 z-50 flex flex-col bg-background pt-[env(safe-area-inset-top)] pb-[env(safe-area-inset-bottom)] animate-fade-in", children: [
      /* @__PURE__ */ jsxs("header", { className: "flex items-center gap-3 border-b border-border bg-surface px-4 py-3", children: [
        /* @__PURE__ */ jsx(IconButton, { variant: "outline", size: "sm", onClick: () => onOpenChange(false), "aria-label": "Back", children: /* @__PURE__ */ jsx(ArrowLeft, {}) }),
        /* @__PURE__ */ jsx("h2", { className: "text-[17px] font-bold text-foreground", children: title })
      ] }),
      /* @__PURE__ */ jsx("div", { className: "lx-scroll flex-1 overflow-y-auto p-4", children })
    ] });
  }
  if (resolved === "mobile") {
    return /* @__PURE__ */ jsxs(Sheet, { open, onOpenChange, portalContainer, children: [
      /* @__PURE__ */ jsx(SheetHeader, { title }),
      /* @__PURE__ */ jsx(SheetBody, { children })
    ] });
  }
  return /* @__PURE__ */ jsxs(Drawer, { open, onOpenChange, portalContainer, children: [
    /* @__PURE__ */ jsx(DrawerHeader, { title, onClose: () => onOpenChange(false) }),
    /* @__PURE__ */ jsx(DrawerBody, { children })
  ] });
}

export {
  DetailDisclosure
};
//# sourceMappingURL=chunk-NB4AQREL.js.map