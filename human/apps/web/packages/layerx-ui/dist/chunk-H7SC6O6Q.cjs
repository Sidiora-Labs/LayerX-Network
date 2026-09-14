"use strict";Object.defineProperty(exports, "__esModule", {value: true}); function _nullishCoalesce(lhs, rhsFn) { if (lhs != null) { return lhs; } else { return rhsFn(); } } function _optionalChain(ops) { let lastAccessLHS = undefined; let value = ops[0]; let i = 1; while (i < ops.length) { const op = ops[i]; const fn = ops[i + 1]; i += 2; if ((op === 'optionalAccess' || op === 'optionalCall') && value == null) { return undefined; } if (op === 'access' || op === 'optionalAccess') { lastAccessLHS = value; value = fn(value); } else if (op === 'call' || op === 'optionalCall') { value = fn((...args) => value.call(lastAccessLHS, ...args)); lastAccessLHS = undefined; } } return value; }"use client";


var _chunkI62LU2PGcjs = require('./chunk-I62LU2PG.cjs');


var _chunkJWRSDZ7Hcjs = require('./chunk-JWRSDZ7H.cjs');


var _chunkRA3A4XJ2cjs = require('./chunk-RA3A4XJ2.cjs');


var _chunkMD6ORKN4cjs = require('./chunk-MD6ORKN4.cjs');

// src/components/app-shell.tsx
var _lucidereact = require('lucide-react');
var _jsxruntime = require('react/jsx-runtime');
var DEFAULT_ICONS = {
  home: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.Home, { className: "size-[22px]" }),
  agents: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.Bot, { className: "size-[22px]" }),
  activity: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.Activity, { className: "size-[22px]" }),
  more: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.Grid2x2, { className: "size-[22px]" })
};
function BottomTabBar({
  items,
  activeId,
  onNavigate,
  onFab,
  fabIcon,
  className
}) {
  const left = items.slice(0, 2);
  const right = items.slice(2, 4);
  const Tab = ({ item }) => {
    const active = item.id === activeId;
    return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0,
      "button",
      {
        type: "button",
        onClick: () => _optionalChain([onNavigate, 'optionalCall', _ => _(item.id)]),
        "aria-current": active ? "page" : void 0,
        className: "relative flex flex-col items-center gap-0.5 py-1 outline-none",
        children: [
          /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: _chunkMD6ORKN4cjs.cn.call(void 0, "transition-colors", active ? "text-accent" : "text-faint-foreground"), children: _nullishCoalesce(_nullishCoalesce(item.icon, () => ( DEFAULT_ICONS[item.id])), () => ( /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.Grid2x2, { className: "size-[22px]" }))) }),
          /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
            "span",
            {
              className: _chunkMD6ORKN4cjs.cn.call(void 0,
                "text-[11px] font-semibold transition-colors",
                active ? "text-accent" : "text-faint-foreground"
              ),
              children: item.label
            }
          ),
          !!item.badge && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "absolute -top-1 right-1/2 translate-x-4 inline-flex h-4 min-w-4 items-center justify-center rounded-full bg-destructive px-1 text-[10px] font-bold text-destructive-foreground", children: item.badge })
        ]
      }
    );
  };
  return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0,
    "nav",
    {
      "aria-label": "Primary",
      className: _chunkMD6ORKN4cjs.cn.call(void 0,
        "relative z-30 grid grid-cols-5 items-end border-t border-border bg-surface px-2 pt-1.5 pb-[max(0.5rem,env(safe-area-inset-bottom))]",
        className
      ),
      children: [
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, Tab, { item: left[0] }),
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, Tab, { item: left[1] }),
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { className: "flex justify-center", children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
          "button",
          {
            type: "button",
            onClick: onFab,
            "aria-label": "New action",
            className: "-mt-7 inline-flex size-14 items-center justify-center rounded-full bg-accent text-accent-foreground shadow-overlay transition-transform active:scale-95",
            children: _nullishCoalesce(fabIcon, () => ( /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.Plus, { className: "size-6" })))
          }
        ) }),
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, Tab, { item: right[0] }),
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, Tab, { item: right[1] })
      ]
    }
  );
}
function Sidebar({
  items,
  activeId,
  onNavigate,
  logo,
  explorerLabel = "Explorer",
  onExplorer,
  settingsLabel = "Settings",
  onSettings,
  user,
  className
}) {
  return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0,
    "aside",
    {
      className: _chunkMD6ORKN4cjs.cn.call(void 0,
        "flex h-full w-[248px] shrink-0 flex-col border-r border-border bg-surface",
        className
      ),
      children: [
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { className: "flex h-16 items-center px-5 text-lg font-extrabold tracking-tight text-foreground", children: _nullishCoalesce(logo, () => ( /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "span", { className: "flex items-center gap-2", children: [
          /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "inline-flex size-7 items-center justify-center rounded-md bg-primary text-[13px] font-black text-primary-foreground", children: "LX" }),
          "LayerX"
        ] }))) }),
        /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "nav", { "aria-label": "Primary", className: "flex flex-1 flex-col gap-1 px-3 py-2", children: [
          items.map((item) => {
            const active = item.id === activeId;
            return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0,
              "button",
              {
                type: "button",
                onClick: () => _optionalChain([onNavigate, 'optionalCall', _2 => _2(item.id)]),
                "aria-current": active ? "page" : void 0,
                className: _chunkMD6ORKN4cjs.cn.call(void 0,
                  "flex items-center gap-3 rounded-md px-3 py-2.5 text-[15px] font-semibold transition-colors outline-none focus-visible:ring-2 focus-visible:ring-accent/30",
                  active ? "bg-surface-sunken text-foreground" : "text-muted-foreground hover:bg-surface-sunken/60 hover:text-foreground"
                ),
                children: [
                  /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: _chunkMD6ORKN4cjs.cn.call(void 0, "[&_svg]:size-5", active ? "text-accent" : "text-faint-foreground"), children: _nullishCoalesce(_nullishCoalesce(item.icon, () => ( DEFAULT_ICONS[item.id])), () => ( /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.Grid2x2, { className: "size-5" }))) }),
                  /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "flex-1 text-left", children: item.label }),
                  !!item.badge && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "inline-flex h-5 min-w-5 items-center justify-center rounded-full bg-destructive px-1.5 text-xs font-bold text-destructive-foreground", children: item.badge })
                ]
              },
              item.id
            );
          }),
          /* @__PURE__ */ _jsxruntime.jsxs.call(void 0,
            "button",
            {
              type: "button",
              onClick: onExplorer,
              className: "mt-4 flex items-center gap-3 rounded-md px-3 py-2.5 text-[15px] font-semibold text-muted-foreground transition-colors hover:bg-surface-sunken/60 hover:text-foreground",
              children: [
                /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.Compass, { className: "size-5 text-faint-foreground" }),
                /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "flex-1 text-left", children: explorerLabel })
              ]
            }
          )
        ] }),
        /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "border-t border-border p-3", children: [
          /* @__PURE__ */ _jsxruntime.jsxs.call(void 0,
            "button",
            {
              type: "button",
              onClick: onSettings,
              className: "flex w-full items-center gap-3 rounded-md px-3 py-2.5 text-[15px] font-semibold text-muted-foreground transition-colors hover:bg-surface-sunken/60 hover:text-foreground",
              children: [
                /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.Settings, { className: "size-5 text-faint-foreground" }),
                /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "flex-1 text-left", children: settingsLabel })
              ]
            }
          ),
          user && /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "mt-1 flex items-center gap-3 rounded-md px-3 py-2", children: [
            /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkJWRSDZ7Hcjs.Avatar, { alt: user.name, src: user.avatarSrc, size: "sm", tone: "primary" }),
            /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "flex min-w-0 flex-col", children: [
              /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "truncate text-sm font-semibold text-foreground", children: user.name }),
              user.subtitle && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "truncate text-xs text-muted-foreground", children: user.subtitle })
            ] })
          ] })
        ] })
      ]
    }
  );
}
function AppShell({
  nav,
  activeNav,
  onNavigate,
  onPrimaryAction,
  primaryActionLabel = "New",
  primaryActionIcon,
  user,
  onSearch,
  onNotifications,
  notificationCount,
  notificationControl,
  onExplorer,
  onSettings,
  logo,
  title,
  headerActions,
  platform,
  className,
  children
}) {
  const resolved = _chunkI62LU2PGcjs.usePlatform.call(void 0, platform);
  if (resolved === "mobile") {
    return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: _chunkMD6ORKN4cjs.cn.call(void 0, "flex h-dvh flex-col bg-background", className), children: [
      /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "header", { className: "flex items-center gap-3 border-b border-border bg-surface px-4 pt-[max(0.75rem,env(safe-area-inset-top))] pb-3", children: [
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkJWRSDZ7Hcjs.Avatar, { alt: _nullishCoalesce(_optionalChain([user, 'optionalAccess', _3 => _3.name]), () => ( "Account")), src: _optionalChain([user, 'optionalAccess', _4 => _4.avatarSrc]), initials: _optionalChain([user, 'optionalAccess', _5 => _5.initials]), size: "sm", tone: "primary" }),
        /* @__PURE__ */ _jsxruntime.jsxs.call(void 0,
          "button",
          {
            type: "button",
            onClick: onSearch,
            className: "flex h-10 flex-1 items-center gap-2.5 rounded-full border border-border bg-surface px-4 text-[15px] text-faint-foreground transition-colors hover:bg-surface-sunken/50",
            children: [
              /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.Search, { className: "size-4", "aria-hidden": true }),
              "Search"
            ]
          }
        ),
        _nullishCoalesce(notificationControl, () => ( /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "relative", children: [
          /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkRA3A4XJ2cjs.IconButton, { variant: "outline", size: "sm", onClick: onNotifications, "aria-label": "Notifications", children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.Bell, {}) }),
          !!notificationCount && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "absolute -top-0.5 -right-0.5 inline-flex h-4 min-w-4 items-center justify-center rounded-full bg-destructive px-1 text-[10px] font-bold text-destructive-foreground", children: notificationCount })
        ] })))
      ] }),
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "main", { className: "lx-scroll flex-1 overflow-y-auto", children }),
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
        BottomTabBar,
        {
          items: nav,
          activeId: activeNav,
          onNavigate,
          onFab: onPrimaryAction,
          fabIcon: primaryActionIcon
        }
      )
    ] });
  }
  return /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: _chunkMD6ORKN4cjs.cn.call(void 0, "flex h-dvh bg-background", className), children: [
    /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
      Sidebar,
      {
        items: nav,
        activeId: activeNav,
        onNavigate,
        logo,
        onExplorer,
        onSettings,
        user: user ? { name: user.name, subtitle: "LayerX account", avatarSrc: user.avatarSrc } : void 0
      }
    ),
    /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "flex min-w-0 flex-1 flex-col", children: [
      /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "header", { className: "flex h-16 shrink-0 items-center gap-4 border-b border-border bg-surface px-6", children: [
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "h1", { className: "text-lg font-bold text-foreground", children: title }),
        /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "div", { className: "flex-1" }),
        /* @__PURE__ */ _jsxruntime.jsxs.call(void 0,
          "button",
          {
            type: "button",
            onClick: onSearch,
            className: "flex h-10 w-64 items-center gap-2.5 rounded-full border border-border bg-surface px-4 text-sm text-faint-foreground transition-colors hover:bg-surface-sunken/50",
            children: [
              /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.Search, { className: "size-4", "aria-hidden": true }),
              /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "flex-1 text-left", children: "Search" }),
              /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "kbd", { className: "rounded border border-border bg-surface-sunken px-1.5 py-0.5 text-[10px] font-semibold text-muted-foreground", children: "\u2318K" })
            ]
          }
        ),
        _nullishCoalesce(notificationControl, () => ( /* @__PURE__ */ _jsxruntime.jsxs.call(void 0, "div", { className: "relative", children: [
          /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _chunkRA3A4XJ2cjs.IconButton, { variant: "outline", size: "sm", onClick: onNotifications, "aria-label": "Notifications", children: /* @__PURE__ */ _jsxruntime.jsx.call(void 0, _lucidereact.Bell, {}) }),
          !!notificationCount && /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { className: "absolute -top-0.5 -right-0.5 inline-flex h-4 min-w-4 items-center justify-center rounded-full bg-destructive px-1 text-[10px] font-bold text-destructive-foreground", children: notificationCount })
        ] }))),
        headerActions
      ] }),
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "main", { className: "lx-scroll flex-1 overflow-y-auto", children })
    ] })
  ] });
}





exports.BottomTabBar = BottomTabBar; exports.Sidebar = Sidebar; exports.AppShell = AppShell;
//# sourceMappingURL=chunk-H7SC6O6Q.cjs.map