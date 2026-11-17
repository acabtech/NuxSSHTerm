// Inline SVG icon set (ported from plans/App.jsx). Stroke uses currentColor.

export type IconName =
  | "folder"
  | "server"
  | "terminal"
  | "settings"
  | "transfer"
  | "key"
  | "search"
  | "lock"
  | "plus"
  | "chev"
  | "splitH"
  | "splitV"
  | "close";

export function Icon({ name, size = 16 }: { name: IconName; size?: number }) {
  const c = {
    width: size,
    height: size,
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: "1.8",
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
    "aria-hidden": true,
  };
  switch (name) {
    case "folder":
      return (
        <svg {...c}>
          <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
        </svg>
      );
    case "server":
      return (
        <svg {...c}>
          <rect x="3" y="4" width="18" height="6" rx="2" />
          <rect x="3" y="14" width="18" height="6" rx="2" />
          <path d="M7 7h.01M7 17h.01" />
        </svg>
      );
    case "terminal":
      return (
        <svg {...c}>
          <path d="M4 17l6-5-6-5" />
          <path d="M12 19h8" />
        </svg>
      );
    case "settings":
      return (
        <svg {...c}>
          <path d="M12 15.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7z" />
          <path d="M19.4 15a1 1 0 0 0 .2 1.1l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1 1 0 0 0-1.1-.2 1 1 0 0 0-.6.9V20a2 2 0 1 1-4 0v-.1a1 1 0 0 0-.7-.9 1 1 0 0 0-1.1.2l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1 1 0 0 0 .2-1.1 1 1 0 0 0-.9-.6H4a2 2 0 1 1 0-4h.1a1 1 0 0 0 .9-.7 1 1 0 0 0-.2-1.1l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1 1 0 0 0 1.1.2h.1a1 1 0 0 0 .6-.9V4a2 2 0 1 1 4 0v.1a1 1 0 0 0 .6.9 1 1 0 0 0 1.1-.2l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1 1 0 0 0-.2 1.1v.1a1 1 0 0 0 .9.6H20a2 2 0 1 1 0 4h-.1a1 1 0 0 0-.9.6z" />
        </svg>
      );
    case "transfer":
      return (
        <svg {...c}>
          <path d="M7 7h11l-3-3m3 3l-3 3" />
          <path d="M17 17H6l3 3m-3-3l3-3" />
        </svg>
      );
    case "key":
      return (
        <svg {...c}>
          <circle cx="8" cy="15" r="4" />
          <path d="M10.8 12.2L20 3v4h-4v4h-3" />
        </svg>
      );
    case "search":
      return (
        <svg {...c}>
          <circle cx="11" cy="11" r="7" />
          <path d="M20 20l-3.5-3.5" />
        </svg>
      );
    case "lock":
      return (
        <svg {...c}>
          <rect x="4" y="11" width="16" height="9" rx="2" />
          <path d="M8 11V8a4 4 0 1 1 8 0v3" />
        </svg>
      );
    case "plus":
      return (
        <svg {...c}>
          <path d="M12 5v14M5 12h14" />
        </svg>
      );
    case "chev":
      return (
        <svg {...c}>
          <path d="M6 9l6 6 6-6" />
        </svg>
      );
    case "splitH":
      return (
        <svg {...c}>
          <rect x="3" y="4" width="18" height="16" rx="2" />
          <path d="M12 4v16" />
        </svg>
      );
    case "splitV":
      return (
        <svg {...c}>
          <rect x="3" y="4" width="18" height="16" rx="2" />
          <path d="M3 12h18" />
        </svg>
      );
    case "close":
      return (
        <svg {...c}>
          <path d="M6 6l12 12M18 6L6 18" />
        </svg>
      );
    default:
      return null;
  }
}
