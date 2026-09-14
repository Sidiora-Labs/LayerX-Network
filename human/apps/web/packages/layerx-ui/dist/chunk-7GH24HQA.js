"use client";
import {
  usePlatform
} from "./chunk-XORHQGZG.js";
import {
  IconButton
} from "./chunk-4X2DK7Y3.js";
import {
  cn
} from "./chunk-LXFZWLUU.js";

// src/components/search.tsx
import * as React from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { Command } from "cmdk";
import { ArrowLeft, Clock, Search } from "lucide-react";
import { jsx, jsxs } from "react/jsx-runtime";
function CommandBar({
  open,
  onOpenChange,
  groups,
  onSelect,
  placeholder = "Search agents, transactions, actions\u2026",
  portalContainer
}) {
  return /* @__PURE__ */ jsx(Dialog.Root, { open, onOpenChange, children: /* @__PURE__ */ jsxs(Dialog.Portal, { container: portalContainer ?? void 0, children: [
    /* @__PURE__ */ jsx(Dialog.Overlay, { className: "fixed inset-0 z-40 bg-black/40 data-[state=open]:animate-fade-in" }),
    /* @__PURE__ */ jsxs(
      Dialog.Content,
      {
        className: cn(
          "fixed top-[18%] left-1/2 z-50 w-[calc(100vw-2rem)] max-w-[560px] -translate-x-1/2",
          "overflow-hidden rounded-xl bg-surface shadow-overlay outline-none",
          "data-[state=open]:animate-fade-in"
        ),
        children: [
          /* @__PURE__ */ jsx(Dialog.Title, { className: "sr-only", children: "Search" }),
          /* @__PURE__ */ jsxs(Command, { label: "Global search", className: "flex flex-col", children: [
            /* @__PURE__ */ jsxs("div", { className: "flex items-center gap-3 border-b border-border px-4", children: [
              /* @__PURE__ */ jsx(Search, { className: "size-[18px] shrink-0 text-muted-foreground", "aria-hidden": true }),
              /* @__PURE__ */ jsx(
                Command.Input,
                {
                  autoFocus: true,
                  placeholder,
                  className: "h-14 w-full bg-transparent text-[15px] text-foreground outline-none placeholder:text-faint-foreground"
                }
              ),
              /* @__PURE__ */ jsx("kbd", { className: "shrink-0 rounded border border-border bg-surface-sunken px-1.5 py-0.5 text-[10px] font-semibold text-muted-foreground", children: "ESC" })
            ] }),
            /* @__PURE__ */ jsxs(Command.List, { className: "lx-scroll max-h-[320px] overflow-y-auto p-2", children: [
              /* @__PURE__ */ jsx(Command.Empty, { className: "py-10 text-center text-sm text-muted-foreground", children: "No results found." }),
              groups.map((g) => /* @__PURE__ */ jsx(
                Command.Group,
                {
                  heading: g.label,
                  className: "[&_[cmdk-group-heading]]:px-3 [&_[cmdk-group-heading]]:py-1.5 [&_[cmdk-group-heading]]:text-xs [&_[cmdk-group-heading]]:font-bold [&_[cmdk-group-heading]]:tracking-wide [&_[cmdk-group-heading]]:text-faint-foreground [&_[cmdk-group-heading]]:uppercase",
                  children: g.items.map((item) => /* @__PURE__ */ jsxs(
                    Command.Item,
                    {
                      value: `${item.title} ${item.subtitle ?? ""} ${(item.keywords ?? []).join(" ")}`,
                      onSelect: () => {
                        onSelect?.(item);
                        onOpenChange(false);
                      },
                      className: "flex cursor-pointer items-center gap-3 rounded-md px-3 py-2.5 data-[selected=true]:bg-surface-sunken",
                      children: [
                        item.icon && /* @__PURE__ */ jsx("span", { className: "inline-flex size-9 shrink-0 items-center justify-center rounded-full bg-surface-sunken text-foreground-secondary [&_svg]:size-4", children: item.icon }),
                        /* @__PURE__ */ jsxs("span", { className: "flex min-w-0 flex-col", children: [
                          /* @__PURE__ */ jsx("span", { className: "truncate text-sm font-semibold text-foreground", children: item.title }),
                          item.subtitle && /* @__PURE__ */ jsx("span", { className: "truncate text-xs text-muted-foreground", children: item.subtitle })
                        ] })
                      ]
                    },
                    item.id
                  ))
                },
                g.id
              ))
            ] })
          ] })
        ]
      }
    )
  ] }) });
}
function SearchScreen({
  open,
  onOpenChange,
  groups,
  onSelect,
  recents,
  placeholder = "Search"
}) {
  const [query, setQuery] = React.useState("");
  React.useEffect(() => {
    if (open) setQuery("");
  }, [open]);
  if (!open) return null;
  const q = query.trim().toLowerCase();
  const matches = (item) => !q || item.title.toLowerCase().includes(q) || item.subtitle?.toLowerCase().includes(q) || item.keywords?.some((k) => k.toLowerCase().includes(q));
  const shownGroups = groups.map((g) => ({ ...g, items: g.items.filter(matches) })).filter((g) => g.items.length > 0);
  return /* @__PURE__ */ jsxs("div", { className: "fixed inset-0 z-50 flex flex-col bg-background animate-fade-in", children: [
    /* @__PURE__ */ jsxs("header", { className: "flex items-center gap-3 border-b border-border bg-surface px-4 py-3", children: [
      /* @__PURE__ */ jsx(IconButton, { variant: "outline", size: "sm", onClick: () => onOpenChange(false), "aria-label": "Back", children: /* @__PURE__ */ jsx(ArrowLeft, {}) }),
      /* @__PURE__ */ jsxs("div", { className: "flex h-10 flex-1 items-center gap-2.5 rounded-full border border-border bg-surface px-4 focus-within:border-accent focus-within:ring-2 focus-within:ring-accent/20", children: [
        /* @__PURE__ */ jsx(Search, { className: "size-4 shrink-0 text-muted-foreground", "aria-hidden": true }),
        /* @__PURE__ */ jsx(
          "input",
          {
            autoFocus: true,
            value: query,
            onChange: (e) => setQuery(e.target.value),
            placeholder,
            className: "w-full bg-transparent text-[15px] text-foreground outline-none placeholder:text-faint-foreground"
          }
        )
      ] })
    ] }),
    /* @__PURE__ */ jsxs("div", { className: "lx-scroll flex-1 overflow-y-auto p-4", children: [
      !q && recents && recents.length > 0 && /* @__PURE__ */ jsxs("section", { children: [
        /* @__PURE__ */ jsx("h4", { className: "pb-1 text-xs font-bold tracking-wide text-faint-foreground uppercase", children: "Recent" }),
        recents.map((item) => /* @__PURE__ */ jsxs(
          "button",
          {
            type: "button",
            onClick: () => {
              onSelect?.(item);
              onOpenChange(false);
            },
            className: "flex w-full items-center gap-3 rounded-md py-2.5 text-left",
            children: [
              /* @__PURE__ */ jsx("span", { className: "inline-flex size-9 shrink-0 items-center justify-center rounded-full bg-surface-sunken text-muted-foreground [&_svg]:size-4", children: item.icon ?? /* @__PURE__ */ jsx(Clock, {}) }),
              /* @__PURE__ */ jsxs("span", { className: "min-w-0 flex-1", children: [
                /* @__PURE__ */ jsx("span", { className: "block truncate text-[15px] font-semibold text-foreground", children: item.title }),
                item.subtitle && /* @__PURE__ */ jsx("span", { className: "block truncate text-[13px] text-muted-foreground", children: item.subtitle })
              ] })
            ]
          },
          item.id
        ))
      ] }),
      shownGroups.map((g) => /* @__PURE__ */ jsxs("section", { className: "pt-3", children: [
        /* @__PURE__ */ jsx("h4", { className: "pb-1 text-xs font-bold tracking-wide text-faint-foreground uppercase", children: g.label }),
        g.items.map((item) => /* @__PURE__ */ jsxs(
          "button",
          {
            type: "button",
            onClick: () => {
              onSelect?.(item);
              onOpenChange(false);
            },
            className: "flex w-full items-center gap-3 rounded-md py-2.5 text-left",
            children: [
              item.icon && /* @__PURE__ */ jsx("span", { className: "inline-flex size-9 shrink-0 items-center justify-center rounded-full bg-surface-sunken text-foreground-secondary [&_svg]:size-4", children: item.icon }),
              /* @__PURE__ */ jsxs("span", { className: "min-w-0 flex-1", children: [
                /* @__PURE__ */ jsx("span", { className: "block truncate text-[15px] font-semibold text-foreground", children: item.title }),
                item.subtitle && /* @__PURE__ */ jsx("span", { className: "block truncate text-[13px] text-muted-foreground", children: item.subtitle })
              ] })
            ]
          },
          item.id
        ))
      ] }, g.id)),
      q && shownGroups.length === 0 && /* @__PURE__ */ jsxs("p", { className: "py-10 text-center text-sm text-muted-foreground", children: [
        "No results for \u201C",
        query,
        "\u201D."
      ] })
    ] })
  ] });
}
function GlobalSearch({
  open,
  onOpenChange,
  groups,
  onSelect,
  recents,
  placeholder,
  enableHotkey = true,
  platform,
  portalContainer
}) {
  const resolved = usePlatform(platform);
  React.useEffect(() => {
    if (!enableHotkey || resolved !== "desktop") return;
    const onKey = (e) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        onOpenChange(!open);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [enableHotkey, resolved, open, onOpenChange]);
  return resolved === "mobile" ? /* @__PURE__ */ jsx(
    SearchScreen,
    {
      open,
      onOpenChange,
      groups,
      onSelect,
      recents,
      placeholder
    }
  ) : /* @__PURE__ */ jsx(
    CommandBar,
    {
      open,
      onOpenChange,
      groups,
      onSelect,
      placeholder,
      portalContainer
    }
  );
}

export {
  GlobalSearch
};
//# sourceMappingURL=chunk-7GH24HQA.js.map