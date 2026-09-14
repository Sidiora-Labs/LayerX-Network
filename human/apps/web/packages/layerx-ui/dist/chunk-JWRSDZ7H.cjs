"use strict";Object.defineProperty(exports, "__esModule", {value: true}); function _interopRequireWildcard(obj) { if (obj && obj.__esModule) { return obj; } else { var newObj = {}; if (obj != null) { for (var key in obj) { if (Object.prototype.hasOwnProperty.call(obj, key)) { newObj[key] = obj[key]; } } } newObj.default = obj; return newObj; } } function _nullishCoalesce(lhs, rhsFn) { if (lhs != null) { return lhs; } else { return rhsFn(); } }"use client";


var _chunkMD6ORKN4cjs = require('./chunk-MD6ORKN4.cjs');

// src/components/avatar.tsx
var _react = require('react'); var React = _interopRequireWildcard(_react);
var _classvarianceauthority = require('class-variance-authority');
var _jsxruntime = require('react/jsx-runtime');
var avatarVariants = _classvarianceauthority.cva.call(void 0,
  "relative inline-flex shrink-0 items-center justify-center overflow-hidden rounded-full font-semibold select-none",
  {
    variants: {
      size: {
        xs: "size-7 text-[11px]",
        sm: "size-9 text-xs",
        md: "size-11 text-sm",
        lg: "size-14 text-base",
        xl: "size-20 text-xl"
      },
      tone: {
        /** Black tile with white initials — the design set's profile avatar. */
        primary: "bg-primary text-primary-foreground",
        accent: "bg-accent-soft text-accent-strong",
        neutral: "bg-surface-sunken text-foreground-secondary"
      }
    },
    defaultVariants: { size: "md", tone: "neutral" }
  }
);
function deriveInitials(name) {
  if (!name) return "";
  return name.split(" ").filter(Boolean).slice(0, 2).map((p) => p[0].toUpperCase()).join("");
}
var Avatar = React.forwardRef(
  ({ className, size, tone, src, alt, initials, ...props }, ref) => {
    const [imgFailed, setImgFailed] = React.useState(false);
    const showImage = src && !imgFailed;
    return /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { ref, className: _chunkMD6ORKN4cjs.cn.call(void 0, avatarVariants({ size, tone, className })), ...props, children: showImage ? (
      // eslint-disable-next-line @next/next/no-img-element
      /* @__PURE__ */ _jsxruntime.jsx.call(void 0,
        "img",
        {
          src,
          alt: _nullishCoalesce(alt, () => ( "")),
          className: "absolute inset-0 size-full object-cover",
          onError: () => setImgFailed(true)
        }
      )
    ) : /* @__PURE__ */ _jsxruntime.jsx.call(void 0, "span", { "aria-hidden": true, children: _nullishCoalesce(initials, () => ( deriveInitials(alt))) }) });
  }
);
Avatar.displayName = "Avatar";




exports.avatarVariants = avatarVariants; exports.Avatar = Avatar;
//# sourceMappingURL=chunk-JWRSDZ7H.cjs.map