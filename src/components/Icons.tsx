/** Minimal inline icon set (no icon-font dependency). */

interface IconProps {
  size?: number;
  className?: string;
}

const base = (size: number) => ({
  width: size,
  height: size,
  viewBox: "0 0 16 16",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.4,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
});

export const FileIcon = ({ size = 14, className }: IconProps) => (
  <svg {...base(size)} className={className}>
    <path d="M9 1.5H4.5A1.5 1.5 0 0 0 3 3v10A1.5 1.5 0 0 0 4.5 14.5h7A1.5 1.5 0 0 0 13 13V5.5L9 1.5Z" />
    <path d="M9 1.5V5.5H13" />
  </svg>
);

export const FolderIcon = ({ size = 14, className }: IconProps) => (
  <svg {...base(size)} className={className}>
    <path d="M2 4.2A1.2 1.2 0 0 1 3.2 3h2.4l1.3 1.6h5.9A1.2 1.2 0 0 1 14 5.8v6A1.2 1.2 0 0 1 12.8 13H3.2A1.2 1.2 0 0 1 2 11.8Z" />
  </svg>
);

export const ImageIcon = ({ size = 14, className }: IconProps) => (
  <svg {...base(size)} className={className}>
    <rect x="2" y="3" width="12" height="10" rx="1.6" />
    <circle cx="6" cy="6.6" r="1.1" />
    <path d="M13.4 10.5 10 7.6l-4.6 4.6" />
  </svg>
);

export const BookIcon = ({ size = 14, className }: IconProps) => (
  <svg {...base(size)} className={className}>
    <path d="M3 2.8A1.3 1.3 0 0 1 4.3 1.5h7.4A1.3 1.3 0 0 1 13 2.8v10.4a.5.5 0 0 1-.77.42L8 10.9 3.77 13.6A.5.5 0 0 1 3 13.2Z" />
  </svg>
);

export const ChevronIcon = ({ size = 12, className }: IconProps) => (
  <svg {...base(size)} className={className}>
    <path d="m4 6 4 4 4-4" />
  </svg>
);

export const ChevronRightIcon = ({ size = 12, className }: IconProps) => (
  <svg {...base(size)} className={className}>
    <path d="m6 4 4 4-4 4" />
  </svg>
);

export const ExportIcon = ({ size = 14, className }: IconProps) => (
  <svg {...base(size)} className={className}>
    <path d="M8 10.5V2.5" />
    <path d="m5 5.5 3-3 3 3" />
    <path d="M2.5 10v2.5A1.5 1.5 0 0 0 4 14h8a1.5 1.5 0 0 0 1.5-1.5V10" />
  </svg>
);

export const FolderOpenIcon = ({ size = 14, className }: IconProps) => (
  <svg {...base(size)} className={className}>
    <path d="M2 12.4V4.2A1.2 1.2 0 0 1 3.2 3h2.4l1.3 1.6h5.9A1.2 1.2 0 0 1 14 5.8v.9" />
    <path d="M2 12.4 3.6 7.4h11.2L13.2 12.4a1.2 1.2 0 0 1-1.14.85H3.2A1.2 1.2 0 0 1 2 12.4Z" />
  </svg>
);

export const RefreshIcon = ({ size = 14, className }: IconProps) => (
  <svg {...base(size)} className={className}>
    <path d="M13.5 8a5.5 5.5 0 1 1-1.63-3.9" />
    <path d="M13.5 2.5V5.5H10.5" />
  </svg>
);

export const PanelIcon = ({ size = 14, className }: IconProps) => (
  <svg {...base(size)} className={className}>
    <rect x="1.8" y="2.8" width="12.4" height="10.4" rx="1.6" />
    <path d="M6.2 2.8v10.4" />
  </svg>
);

export const InfoIcon = ({ size = 14, className }: IconProps) => (
  <svg {...base(size)} className={className}>
    <circle cx="8" cy="8" r="6.2" />
    <path d="M8 7.4v3.4M8 5.3h.01" />
  </svg>
);

export const WarnIcon = ({ size = 14, className }: IconProps) => (
  <svg {...base(size)} className={className}>
    <path d="M8 2.2 14.2 13H1.8Z" />
    <path d="M8 6.4v3M8 11.2h.01" />
  </svg>
);

export const CheckIcon = ({ size = 14, className }: IconProps) => (
  <svg {...base(size)} className={className}>
    <path d="m3.5 8.5 3 3 6-7" />
  </svg>
);

export const CloseIcon = ({ size = 14, className }: IconProps) => (
  <svg {...base(size)} className={className}>
    <path d="m4 4 8 8M12 4l-8 8" />
  </svg>
);

export const RestoreIcon = ({ size = 14, className }: IconProps) => (
  <svg {...base(size)} className={className}>
    <path d="M3 8a5 5 0 1 0 1.6-3.7" />
    <path d="M3 2.6V5.6h3" />
  </svg>
);

export const SunIcon = ({ size = 14, className }: IconProps) => (
  <svg {...base(size)} className={className}>
    <circle cx="8" cy="8" r="3.1" />
    <path d="M8 1.4v1.7M8 12.9v1.7M1.4 8h1.7M12.9 8h1.7M3.4 3.4l1.2 1.2M11.4 11.4l1.2 1.2M12.6 3.4l-1.2 1.2M4.6 11.4l-1.2 1.2" />
  </svg>
);

export const MoonIcon = ({ size = 14, className }: IconProps) => (
  <svg {...base(size)} className={className}>
    <path d="M13.4 9.6A5.6 5.6 0 0 1 6.4 2.6a5.6 5.6 0 1 0 7 7Z" />
  </svg>
);
