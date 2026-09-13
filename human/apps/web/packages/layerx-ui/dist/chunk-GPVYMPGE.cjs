"use strict";Object.defineProperty(exports, "__esModule", {value: true}); function _interopRequireWildcard(obj) { if (obj && obj.__esModule) { return obj; } else { var newObj = {}; if (obj != null) { for (var key in obj) { if (Object.prototype.hasOwnProperty.call(obj, key)) { newObj[key] = obj[key]; } } } newObj.default = obj; return newObj; } } function _nullishCoalesce(lhs, rhsFn) { if (lhs != null) { return lhs; } else { return rhsFn(); } } function _optionalChain(ops) { let lastAccessLHS = undefined; let value = ops[0]; let i = 1; while (i < ops.length) { const op = ops[i]; const fn = ops[i + 1]; i += 2; if ((op === 'optionalAccess' || op === 'optionalCall') && value == null) { return undefined; } if (op === 'access' || op === 'optionalAccess') { lastAccessLHS = value; value = fn(value); } else if (op === 'call' || op === 'optionalCall') { value = fn((...args) => value.call(lastAccessLHS, ...args)); lastAccessLHS = undefined; } } return value; }"use client";


var _chunkI62LU2PGcjs = require('./chunk-I62LU2PG.cjs');


var _chunkRA3A4XJ2cjs = require('./chunk-RA3A4XJ2.cjs');


var _chunkMD6ORKN4cjs = require('./chunk-MD6ORKN4.cjs');

// src/components/search.tsx
var _react = require('react'); var React = _interopRequireWildcard(_react);
var _reactdialog = require('@radix-ui/react-dialog'); var Dialog = _interopRequireWildcard(_reactdialog);
var _cmdk = require('cmdk');
var _lucidereact = require('lucide-react');
var _jsxruntime = require('react/jsx-runtime');
function CommandBar({
  open,
  onOpenChange,
  groups,
  onSelect,
  placeholder = "Search agents, transactions, actions\u2026",
  portalContainer
}) {
  return /* @__PURE__ */ _jsxruntime.jsx.call(void 0, Dialog.Root, { open, onOpenChange, children: /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, Dialog.Portal, { container: _nullishCoalesce(portalContainer, () => ( void 0)), children: [
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0, Dialog.Overlay, { className: "fixed inset-0 z-40 bg-black/40 data-[state=open]:animate-fade-in" }),
    /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, 
      Dialog.Content,
      {
        className: _chunkMD6ORKN4cjs.cn.call(void 0, 
          "fixed top-[18%] left-1/2 z-50 w-[calc(100vw-2rem)] max-w-[560px] -translate-x-1/2",
          "overflow-hidden rounded-xl bg-surface shadow-overlay outline-none",
          "data-[state=open]:animate-fade-in"
        ),
        children: [
          /* @__PURE__ */ _jsxruntime.jsx.call(void 0, Dialog.Title, { className: "sr-only", children: "Search" }),
          /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, _cmdk.Command, { label: "Global search", className: "flex flex-col", children: [
            /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "flex items-center gap-3 border-b border-border px-4", children: [
              /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.Search, { className: "size-[18px] shrink-0 text-muted-foreground", "aria-hidden": true }),
              /* @__PURE__ */ _jsxruntime.jsx.call(void 0, 
                _cmdk.Command.Input,
                {
                  autoFocus: true,
                  placeholder,
                  className: "h-14 w-full bg-transparent text-[15px] text-foreground outline-none placeholder:text-faint-foreground"
                }
              ),
              /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "kbd", { className: "shrink-0 rounded border border-border bg-surface-sunken px-1.5 py-0.5 text-[10px] font-semibold text-muted-foreground", children: "ESC" })
            ] }),
            /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, _cmdk.Command.List, { className: "lx-scroll max-h-[320px] overflow-y-auto p-2", children: [
              /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _cmdk.Command.Empty, { className: "py-10 text-center text-sm text-muted-foreground", children: "No results found." }),
              groups.map((g) => /* @__PURE__ */ _jsxruntime.jsx.call(void 0, 
                _cmdk.Command.Group,
                {
                  heading: g.label,
                  className: "[&_[cmdk-group-heading]]:px-3 [&_[cmdk-group-heading]]:py-1.5 [&_[cmdk-group-heading]]:text-xs [&_[cmdk-group-heading]]:font-bold [&_[cmdk-group-heading]]:tracking-wide [&_[cmdk-group-heading]]:text-faint-foreground [&_[cmdk-group-heading]]:uppercase",
                  children: g.items.map((item) => /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, 
                    _cmdk.Command.Item,
                    {
                      value: `${item.title} ${_nullishCoalesce(item.subtitle, () => ( ""))} ${(_nullishCoalesce(item.keywords, () => ( []))).join(" ")}`,
                      onSelect: () => {
                        _optionalChain([onSelect, 'optionalCall', _ => _(item)]);
                        onOpenChange(false);
                      },
                      className: "flex cursor-pointer items-center gap-3 rounded-md px-3 py-2.5 data-[selected=true]:bg-surface-sunken",
                      children: [
                        item.icon && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "inline-flex size-9 shrink-0 items-center justify-center rounded-full bg-surface-sunken text-foreground-secondary [&_svg]:size-4", children: item.icon }),
                        /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "span", { className: "flex min-w-0 flex-col", children: [
                          /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "truncate text-sm font-semibold text-foreground", children: item.title }),
                          item.subtitle && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "truncate text-xs text-muted-foreground", children: item.subtitle })
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
  const matches = (item) => !q || item.title.toLowerCase().includes(q) || _optionalChain([item, 'access', _2 => _2.subtitle, 'optionalAccess', _3 => _3.toLowerCase, 'call', _4 => _4(), 'access', _5 => _5.includes, 'call', _6 => _6(q)]) || _optionalChain([item, 'access', _7 => _7.keywords, 'optionalAccess', _8 => _8.some, 'call', _9 => _9((k) => k.toLowerCase().includes(q))]);
  const shownGroups = groups.map((g) => ({ ...g, items: g.items.filter(matches) })).filter((g) => g.items.length > 0);
  return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "fixed inset-0 z-50 flex flex-col bg-background animate-fade-in", children: [
    /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "header", { className: "flex items-center gap-3 border-b border-border bg-surface px-4 py-3", children: [
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkRA3A4XJ2cjs.IconButton, { variant: "outline", size: "sm", onClick: () => onOpenChange(false), "aria-label": "Back", children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.ArrowLeft, {}) }),
      /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "flex h-10 flex-1 items-center gap-2.5 rounded-full border border-border bg-surface px-4 focus-within:border-accent focus-within:ring-2 focus-within:ring-accent/20", children: [
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.Search, { className: "size-4 shrink-0 text-muted-foreground", "aria-hidden": true }),
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, 
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
    /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "lx-scroll flex-1 overflow-y-auto p-4", children: [
      !q && recents && recents.length > 0 && /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "section", { children: [
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "h4", { className: "pb-1 text-xs font-bold tracking-wide text-faint-foreground uppercase", children: "Recent" }),
        recents.map((item) => /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, 
          "button",
          {
            type: "button",
            onClick: () => {
              _optionalChain([onSelect, 'optionalCall', _10 => _10(item)]);
              onOpenChange(false);
            },
            className: "flex w-full items-center gap-3 rounded-md py-2.5 text-left",
            children: [
              /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "inline-flex size-9 shrink-0 items-center justify-center rounded-full bg-surface-sunken text-muted-foreground [&_svg]:size-4", children: _nullishCoalesce(item.icon, () => ( /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.Clock, {}))) }),
              /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "span", { className: "min-w-0 flex-1", children: [
                /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "block truncate text-[15px] font-semibold text-foreground", children: item.title }),
                item.subtitle && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "block truncate text-[13px] text-muted-foreground", children: item.subtitle })
              ] })
            ]
          },
          item.id
        ))
      ] }),
      shownGroups.map((g) => /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "section", { className: "pt-3", children: [
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "h4", { className: "pb-1 text-xs font-bold tracking-wide text-faint-foreground uppercase", children: g.label }),
        g.items.map((item) => /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, 
          "button",
          {
            type: "button",
            onClick: () => {
              _optionalChain([onSelect, 'optionalCall', _11 => _11(item)]);
              onOpenChange(false);
            },
            className: "flex w-full items-center gap-3 rounded-md py-2.5 text-left",
            children: [
              item.icon && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "inline-flex size-9 shrink-0 items-center justify-center rounded-full bg-surface-sunken text-foreground-secondary [&_svg]:size-4", children: item.icon }),
              /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "span", { className: "min-w-0 flex-1", children: [
                /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "block truncate text-[15px] font-semibold text-foreground", children: item.title }),
                item.subtitle && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "block truncate text-[13px] text-muted-foreground", children: item.subtitle })
              ] })
            ]
          },
          item.id
        ))
      ] }, g.id)),
      q && shownGroups.length === 0 && /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "p", { className: "py-10 text-center text-sm text-muted-foreground", children: [
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
  const resolved = _chunkI62LU2PGcjs.usePlatform.call(void 0, platform);
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
  return resolved === "mobile" ? /* @__PURE__ */ _jsxruntime.jsx.call(void 0, 
    SearchScreen,
    {
      open,
      onOpenChange,
      groups,
      onSelect,
      recents,
      placeholder
    }
  ) : /* @__PURE__ */ _jsxruntime.jsx.call(void 0, 
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



exports.GlobalSearch = GlobalSearch;
//# sourceMappingURL=chunk-GPVYMPGE.cjs.map