import { useId } from "react";

/** The app mark: a bezier segment with its anchors and one handle. Same drawing as public/favicon.svg. */
export function Logo({ className }: { className?: string }) {
  // useId keeps the gradient id unique if the mark appears twice on a page.
  const gradient = useId();
  return (
    <svg viewBox="0 0 32 32" className={className} aria-hidden="true">
      <defs>
        <linearGradient id={gradient} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#5b5bf6" />
          <stop offset="1" stopColor="#b44cf0" />
        </linearGradient>
      </defs>
      <rect width="32" height="32" rx="8" fill={`url(#${gradient})`} />
      <path d="M8.5 22.5 12 9" stroke="#fff" strokeOpacity=".55" strokeWidth="1.25" />
      <path
        d="M8.5 22.5C12 9 20 23 23.5 9.5"
        fill="none"
        stroke="#fff"
        strokeWidth="2.4"
        strokeLinecap="round"
      />
      <rect x="6.5" y="20.5" width="4" height="4" rx="1" fill="#fff" />
      <rect x="21.5" y="7.5" width="4" height="4" rx="1" fill="#fff" />
      <circle cx="12" cy="9" r="1.7" fill="#fff" />
    </svg>
  );
}
