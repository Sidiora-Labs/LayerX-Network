"use client";
import {
  cn
} from "./chunk-LXFZWLUU.js";

// src/components/list.tsx
import { ChevronRight } from "lucide-react";
import { jsx, jsxs } from "react/jsx-runtime";
function List({ className, ...props }) {
  return /* @__PURE__ */ jsx(
    "div",
    {
      className: cn("flex flex-col divide-y divide-border/70", className),
      role: "list",
      ...props
    }
  );
}
function IconTile({
  className,
  tone = "neutral",
  shape = "square",
  ...props
}) {
  const tones = {
    neutral: "bg-surface-sunken text-foreground-secondary",
    accent: "bg-accent-soft text-accent",
    success: "bg-success-soft text-success",
    destructive: "bg-destructive-soft text-destructive"
  };
  return /* @__PURE__ */ jsx(
    "span",
    {
      className: cn(
        "inline-flex size-11 shrink-0 items-center justify-center [&_svg]:size-5",
        shape === "square" ? "rounded-md" : "rounded-full",
        tones[tone],
        className
      ),
      ...props
    }
  );
}
function ListItem({
  className,
  leading,
  title,
  subtitle,
  trailing,
  trailingCaption,
  navigates,
  onClick,
  ...props
}) {
  const interactive = Boolean(onClick) || navigates;
  return /* @__PURE__ */ jsxs(
    "div",
    {
      role: "listitem",
      tabIndex: interactive ? 0 : void 0,
      onClick,
      onKeyDown: onClick ? (e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onClick(e);
        }
      } : void 0,
      className: cn(
        "flex w-full items-center gap-3 py-3.5 text-left",
        interactive && "cursor-pointer transition-colors hover:bg-surface-sunken/40 -mx-2 px-2 rounded-md",
        className
      ),
      ...props,
      children: [
        leading,
        /* @__PURE__ */ jsxs("span", { className: "flex min-w-0 flex-1 flex-col gap-0.5", children: [
          /* @__PURE__ */ jsx("span", { className: "truncate text-[15px] font-semibold text-foreground", children: title }),
          subtitle && /* @__PURE__ */ jsx("span", { className: "truncate text-[13px] text-muted-foreground", children: subtitle })
        ] }),
        (trailing || navigates) && /* @__PURE__ */ jsxs("span", { className: "flex shrink-0 flex-col items-end gap-0.5", children: [
          /* @__PURE__ */ jsxs("span", { className: "flex items-center gap-1.5", children: [
            trailing,
            navigates && /* @__PURE__ */ jsx(ChevronRight, { className: "size-4 text-faint-foreground", "aria-hidden": true })
          ] }),
          trailingCaption && /* @__PURE__ */ jsx("span", { className: "text-xs text-faint-foreground", children: trailingCaption })
        ] })
      ]
    }
  );
}
function SectionHeader({
  title,
  action,
  className
}) {
  return /* @__PURE__ */ jsxs("div", { className: cn("flex items-center justify-between gap-3", className), children: [
    /* @__PURE__ */ jsx("h3", { className: "text-[17px] font-bold text-foreground", children: title }),
    action
  ] });
}
function ViewAllChip({
  className,
  children = "View all",
  ...props
}) {
  return /* @__PURE__ */ jsx(
    "button",
    {
      type: "button",
      className: cn(
        "inline-flex h-8 items-center rounded-full bg-surface border border-border px-3.5 text-[13px] font-semibold text-foreground-secondary transition-colors hover:bg-surface-sunken/60",
        className
      ),
      ...props,
      children
    }
  );
}
function Divider({ className }) {
  return /* @__PURE__ */ jsx("hr", { className: cn("border-0 border-t border-border", className) });
}

export {
  List,
  IconTile,
  ListItem,
  SectionHeader,
  ViewAllChip,
  Divider
};
//# sourceMappingURL=chunk-EP5CBYSP.js.map