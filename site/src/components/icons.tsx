// Inline SVG icons, the set the mockups use. Stroke icons inherit `currentColor`.

type P = { size?: number };

const stroke = (size: number) => ({
  width: size,
  height: size,
  viewBox: "0 0 20 20",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.6,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
  "aria-hidden": true,
});

export const Chevron = ({ size = 18 }: P) => (
  <svg {...stroke(size)}>
    <path d="M7 5l5 5-5 5" />
  </svg>
);
export const Plus = ({ size = 16 }: P) => (
  <svg {...stroke(size)}>
    <path d="M10 4v12M4 10h12" />
  </svg>
);
export const Play = ({ size = 16 }: P) => (
  <svg {...stroke(size)}>
    <path d="M6 4l10 6-10 6z" />
  </svg>
);
export const Stop = ({ size = 16 }: P) => (
  <svg {...stroke(size)}>
    <rect x="5" y="5" width="10" height="10" rx="1.5" />
  </svg>
);
export const Restart = ({ size = 16 }: P) => (
  <svg {...stroke(size)}>
    <path d="M15.5 10a5.5 5.5 0 1 1-1.6-3.9M15.5 4v3h-3" />
  </svg>
);
export const Trash = ({ size = 16 }: P) => (
  <svg {...stroke(size)}>
    <path d="M4 6h12M8 6V4h4v2M6 6l1 10h6l1-10" />
  </svg>
);
export const Link = ({ size = 16 }: P) => (
  <svg {...stroke(size)}>
    <path d="M8.5 11.5l3-3M7 14l-1.5 1.5a2.8 2.8 0 01-4-4L4 10M13 6l1.5-1.5a2.8 2.8 0 014 4L17 10" />
  </svg>
);
export const Key = ({ size = 16 }: P) => (
  <svg {...stroke(size)}>
    <circle cx="7.5" cy="12.5" r="3.5" />
    <path d="M10.5 10L17 3.5M14 6l2 2" />
  </svg>
);
export const Alert = ({ size = 16 }: P) => (
  <svg {...stroke(size)}>
    <path d="M10 3l8 14H2z M10 8v4M10 14.5v.5" />
  </svg>
);
export const Google = ({ size = 18 }: P) => (
  <svg width={size} height={size} viewBox="0 0 18 18" aria-hidden="true">
    <path fill="#4285F4" d="M17.64 9.2c0-.64-.06-1.25-.16-1.84H9v3.48h4.84a4.14 4.14 0 01-1.8 2.72v2.26h2.92c1.7-1.57 2.68-3.88 2.68-6.62z" />
    <path fill="#34A853" d="M9 18c2.43 0 4.47-.8 5.96-2.18l-2.92-2.26c-.8.54-1.84.86-3.04.86-2.34 0-4.32-1.58-5.03-3.7H.96v2.33A9 9 0 009 18z" />
    <path fill="#FBBC05" d="M3.97 10.72A5.4 5.4 0 013.68 9c0-.6.1-1.18.29-1.72V4.95H.96A9 9 0 000 9c0 1.45.35 2.83.96 4.05l3.01-2.33z" />
    <path fill="#EA4335" d="M9 3.58c1.32 0 2.5.45 3.44 1.35l2.58-2.58C13.46.9 11.43 0 9 0A9 9 0 00.96 4.95l3.01 2.33C4.68 5.16 6.66 3.58 9 3.58z" />
  </svg>
);
