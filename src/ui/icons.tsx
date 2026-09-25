// Inline stroke icons that inherit the text color. Decorative ones are hidden
// from screen readers; the caller labels the button around them.

interface IconProps {
  size?: number;
  className?: string;
}

const base = (size: number) => ({
  width: size,
  height: size,
  viewBox: "0 0 24 24",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.8,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
});

export function LogoMark({ size = 22, className }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" className={className} aria-hidden>
      <rect x="3" y="13" width="6" height="5" rx="1.4" fill="currentColor" />
      <rect x="10" y="13" width="6" height="5" rx="1.4" fill="currentColor" />
      <rect x="17" y="13" width="4" height="5" rx="1.4" fill="currentColor" />
      <rect x="6.5" y="7.5" width="6" height="5" rx="1.4" fill="currentColor" opacity="0.7" />
      <rect x="13.5" y="7.5" width="6" height="5" rx="1.4" fill="currentColor" opacity="0.7" />
      <rect x="10" y="2" width="6" height="5" rx="1.4" fill="currentColor" opacity="0.45" />
    </svg>
  );
}

export function PlusIcon({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} aria-hidden>
      <path d="M12 5v14M5 12h14" />
    </svg>
  );
}

export function CheckIcon({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} aria-hidden>
      <path d="m5 12.5 4.5 4.5L19 7" />
    </svg>
  );
}

export function XIcon({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} aria-hidden>
      <path d="M6 6l12 12M18 6 6 18" />
    </svg>
  );
}

export function SpinnerIcon({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={`spin ${className ?? ""}`} aria-hidden>
      <path d="M12 3a9 9 0 1 1-6.4 2.6" />
    </svg>
  );
}

export function PlayIcon({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} aria-hidden>
      <path d="M7 4.5v15l12-7.5z" fill="currentColor" stroke="none" />
    </svg>
  );
}

export function StopIcon({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} aria-hidden>
      <rect x="6" y="6" width="12" height="12" rx="2" fill="currentColor" stroke="none" />
    </svg>
  );
}

export function RefreshIcon({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} aria-hidden>
      <path d="M20 12a8 8 0 1 1-2.3-5.7M20 4v5h-5" />
    </svg>
  );
}

export function CopyIcon({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} aria-hidden>
      <rect x="9" y="9" width="11" height="11" rx="2" />
      <path d="M5 15V6a2 2 0 0 1 2-2h9" />
    </svg>
  );
}

export function TrashIcon({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} aria-hidden>
      <path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13M10 11v6M14 11v6" />
    </svg>
  );
}

export function TerminalIcon({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} aria-hidden>
      <rect x="3" y="4" width="18" height="16" rx="2" />
      <path d="m7 9 3 3-3 3M12 15h5" />
    </svg>
  );
}

export function ExternalIcon({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} aria-hidden>
      <path d="M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5" />
    </svg>
  );
}

export function FolderIcon({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} aria-hidden>
      <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
    </svg>
  );
}

export function LogsIcon({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} aria-hidden>
      <path d="M4 6h16M4 12h10M4 18h13" />
    </svg>
  );
}

export function ActivityIcon({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} aria-hidden>
      <path d="M3 12h4l2 6 4-14 2 8h6" />
    </svg>
  );
}

export function MachineIcon({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} aria-hidden>
      <rect x="4" y="5" width="16" height="11" rx="1.5" />
      <path d="M2 19h20" />
    </svg>
  );
}

export function BookIcon({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} aria-hidden>
      <path d="M4 5a2 2 0 0 1 2-2h13v16H6a2 2 0 0 0-2 2z" />
      <path d="M4 19a2 2 0 0 1 2-2h13" />
    </svg>
  );
}

export function ArrowLeftIcon({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} aria-hidden>
      <path d="M19 12H5M11 6l-6 6 6 6" />
    </svg>
  );
}

export function MoreIcon({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} aria-hidden>
      <circle cx="6" cy="12" r="1.3" fill="currentColor" stroke="none" />
      <circle cx="12" cy="12" r="1.3" fill="currentColor" stroke="none" />
      <circle cx="18" cy="12" r="1.3" fill="currentColor" stroke="none" />
    </svg>
  );
}

export function HelpIcon({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} aria-hidden>
      <circle cx="12" cy="12" r="9" />
      <path d="M9.6 9.3a2.5 2.5 0 0 1 4.8.9c0 1.7-2.4 2.2-2.4 3.6" />
      <path d="M12 17h.01" />
    </svg>
  );
}

export function GearIcon({ size = 14, className }: IconProps) {
  return (
    <svg {...base(size)} className={className} aria-hidden>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" />
    </svg>
  );
}
