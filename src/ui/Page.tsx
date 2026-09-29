import { useId, type KeyboardEvent, type ReactNode } from "react";

import { cx } from "./primitives";

export interface TabItem<Id extends string> {
  id: Id;
  label: string;
  /** A count shown after the label, like the number of stacks. */
  count?: number;
}

export interface PageTabs<Id extends string> {
  items: TabItem<Id>[];
  value: Id;
  onChange: (id: Id) => void;
}

/** Every page starts the same way: a header that stays at the top while the
 * page scrolls, with the page's name, one line about it, its actions and,
 * for pages with several parts, tabs. With tabs, the page's body is the tab
 * panel they control. The header doubles as the window's drag area, because
 * the macOS title bar is drawn over the app. */
export function Page<Id extends string>({
  title,
  summary,
  actions,
  tabs,
  width = "max-w-5xl",
  children,
}: {
  title: ReactNode;
  summary?: ReactNode;
  actions?: ReactNode;
  tabs?: PageTabs<Id>;
  width?: string;
  children: ReactNode;
}) {
  const base = useId();
  const panel = tabs ? { id: `${base}-panel`, role: "tabpanel", "aria-labelledby": tabId(base, tabs.value) } : {};
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
        {tabs ? (
          <div className={cx("px-8", width)}>
            <Tabs base={base} {...tabs} />
          </div>
        ) : null}
      </header>
      <div className={cx("space-y-5 px-8 pt-6 pb-12", width)} {...panel}>
        {children}
      </div>
    </div>
  );
}

function tabId(base: string, id: string): string {
  return `${base}-tab-${id}`;
}

/** Which tab a key moves to, the way tabs work everywhere: the arrows step
 * (and wrap), Home and End jump. Null for any other key. */
function tabAfterKey(key: string, index: number, count: number): number | null {
  if (key === "ArrowRight") return (index + 1) % count;
  if (key === "ArrowLeft") return (index - 1 + count) % count;
  if (key === "Home") return 0;
  if (key === "End") return count - 1;
  return null;
}

/** Tabs under a page's title, for pages whose parts are separate jobs. Only
 * the chosen tab is in the Tab order; the arrow keys move between them and
 * show the one they land on. */
function Tabs<Id extends string>({ base, items, value, onChange }: PageTabs<Id> & { base: string }) {
  const onKeyDown = (event: KeyboardEvent) => {
    const index = items.findIndex((tab) => tab.id === value);
    const target = tabAfterKey(event.key, index, items.length);
    if (target === null) return;
    event.preventDefault();
    const next = items[target];
    onChange(next.id);
    document.getElementById(tabId(base, next.id))?.focus();
  };
  return (
    <div role="tablist" className="-mb-px flex gap-6" onKeyDown={onKeyDown}>
      {items.map((tab) => {
        const selected = tab.id === value;
        return (
          <button
            key={tab.id}
            id={tabId(base, tab.id)}
            type="button"
            role="tab"
            aria-selected={selected}
            aria-controls={`${base}-panel`}
            tabIndex={selected ? 0 : -1}
            onClick={() => onChange(tab.id)}
            className={cx(
              "border-b-2 pt-1 pb-2.5 text-[13px] font-medium transition",
              selected ? "border-accent text-ink" : "border-transparent text-ink-3 hover:text-ink",
            )}
          >
            {tab.label}
            {tab.count !== undefined ? <span className="tabular ml-1.5 text-ink-3">{tab.count}</span> : null}
          </button>
        );
      })}
    </div>
  );
}
