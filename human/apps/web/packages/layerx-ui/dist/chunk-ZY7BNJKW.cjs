"use strict";Object.defineProperty(exports, "__esModule", {value: true});"use client";


var _chunkMD6ORKN4cjs = require('./chunk-MD6ORKN4.cjs');

// src/components/list.tsx
var _lucidereact = require('lucide-react');
var _jsxruntime = require('react/jsx-runtime');
function List({ className, ...props }) {
  return /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
    "div",
    {
      className: _chunkMD6ORKN4cjs.cn.call(void 0, "flex flex-col divide-y divide-border/70", className),
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
  return /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
    "span",
    {
      className: _chunkMD6ORKN4cjs.cn.call(void 0,
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
  return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0,
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
      className: _chunkMD6ORKN4cjs.cn.call(void 0,
        "flex w-full items-center gap-3 py-3.5 text-left",
        interactive && "cursor-pointer transition-colors hover:bg-surface-sunken/40 -mx-2 px-2 rounded-md",
        className
      ),
      ...props,
      children: [
        leading,
        /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "span", { className: "flex min-w-0 flex-1 flex-col gap-0.5", children: [
          /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "truncate text-[15px] font-semibold text-foreground", children: title }),
          subtitle && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "truncate text-[13px] text-muted-foreground", children: subtitle })
        ] }),
        (trailing || navigates) && /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "span", { className: "flex shrink-0 flex-col items-end gap-0.5", children: [
          /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "span", { className: "flex items-center gap-1.5", children: [
            trailing,
            navigates && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.ChevronRight, { className: "size-4 text-faint-foreground", "aria-hidden": true })
          ] }),
          trailingCaption && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "text-xs text-faint-foreground", children: trailingCaption })
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
  return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: _chunkMD6ORKN4cjs.cn.call(void 0, "flex items-center justify-between gap-3", className), children: [
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "h3", { className: "text-[17px] font-bold text-foreground", children: title }),
    action
  ] });
}
function ViewAllChip({
  className,
  children = "View all",
  ...props
}) {
  return /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
    "button",
    {
      type: "button",
      className: _chunkMD6ORKN4cjs.cn.call(void 0,
        "inline-flex h-8 items-center rounded-full bg-surface border border-border px-3.5 text-[13px] font-semibold text-foreground-secondary transition-colors hover:bg-surface-sunken/60",
        className
      ),
      ...props,
      children
    }
  );
}
function Divider({ className }) {
  return /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "hr", { className: _chunkMD6ORKN4cjs.cn.call(void 0, "border-0 border-t border-border", className) });
}








exports.List = List; exports.IconTile = IconTile; exports.ListItem = ListItem; exports.SectionHeader = SectionHeader; exports.ViewAllChip = ViewAllChip; exports.Divider = Divider;
//# sourceMappingURL=chunk-ZY7BNJKW.cjs.map