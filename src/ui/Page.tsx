import type { ReactNode } from "react";

import { cx } from "./primitives";

/** Every page starts the same way: a header that stays at the top while the
 * page scrolls, with the page's name, one line about it, its actions and,
 * for pages with several parts, tabs. The header doubles as the window's
 * drag area, because the macOS title bar is drawn over the app. */
export function Page({ title, summary, actions, tabs, width = "max-w-5xl", children }: { title: ReactNode; summary?: ReactNode; actions?: ReactNode; tabs?: ReactNode; width?: string; children: ReactNode }) {
  return (
    <div className="min-h-full">
      <header data-tauri-drag-region className="sticky top-0 z-10 border-b border-line bg-plane/90 backdrop-blur-md">
        {/* Left-aligned, not centred: every page's title then sits in the same place, and switching pages does not shift it. */}
        <div data-tauri-drag-region className={cx("flex min-h-16 flex-wrap items-center justify-between gap-x-4 gap-y-2 px-8 py-3", width)}>
          <div data-tauri-drag-region className="min-w-0">
            <h1 data-tauri-drag-region className="flex min-w-0 items-center gap-2.5 text-[19px] font-semibold tracking-tight text-ink">
              {title}
            </h1>
            {summary ? (
              <div data-tauri-drag-region className="mt-0.5 truncate text-[12px] text-ink-3">
                {summary}
              </div>
            ) : null}
          </div>
          {actions ? <div className="flex shrink-0 flex-wrap items-center gap-2">{actions}</div> : null}
        </div>
        {tabs ? <div className={cx("px-8", width)}>{tabs}</div> : null}
      </header>
      <div className={cx("space-y-5 px-8 pt-6 pb-12", width)}>{children}</div>
    </div>
  );
}

export interface TabItem<Id extends string> {
  id: Id;
  label: string;
  /** A count shown after the label, like the number of stacks. */
  count?: number;
}

/** Tabs under a page's title, for pages whose parts are separate jobs. */
export function Tabs<Id extends string>({ tabs, value, onChange }: { tabs: TabItem<Id>[]; value: Id; onChange: (id: Id) => void }) {
  return (
    <div role="tablist" className="-mb-px flex gap-6">
      {tabs.map((tab) => {
        const selected = tab.id === value;
        return (
          <button
            key={tab.id}
            type="button"
            role="tab"
            aria-selected={selected}
            onClick={() => onChange(tab.id)}
            className={cx("border-b-2 pt-1 pb-2.5 text-[13px] font-medium transition", selected ? "border-accent text-ink" : "border-transparent text-ink-3 hover:text-ink")}
          >
            {tab.label}
            {tab.count !== undefined ? <span className="tabular ml-1.5 text-ink-3">{tab.count}</span> : null}
          </button>
        );
      })}
    </div>
  );
}
