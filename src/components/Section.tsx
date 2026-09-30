import { useState, type ReactNode } from "react";

import { ChevronIcon } from "./Icons";

interface SectionProps {
  title: string;
  count?: ReactNode;
  defaultOpen?: boolean;
  children: ReactNode;
}

export function Section({ title, count, defaultOpen = true, children }: SectionProps) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <div className="section">
      <button
        type="button"
        className={`section-head${open ? "" : " is-collapsed"}`}
        onClick={() => setOpen((value) => !value)}
      >
        <span className="chevron">
          <ChevronIcon size={11} />
        </span>
        <span>{title}</span>
        {count !== undefined && <span className="section-count">{count}</span>}
      </button>
      {open && <div className="section-body">{children}</div>}
    </div>
  );
}
