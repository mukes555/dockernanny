import { useEffect, useRef } from "react";

// The open panels, the most recently opened last. One Escape closes only
// that one, the way a stack of sheets is taken off from the top.
const openPanels: Array<{ close: () => void }> = [];

// Listens on the window, which hears the key after the document does, so an
// open dialog or menu (they listen on the document and mark the key as
// handled) closes first, alone.
function onKey(event: KeyboardEvent) {
  const handledAbove = event.defaultPrevented;
  const top = openPanels[openPanels.length - 1];
  if (event.key !== "Escape" || handledAbove || !top) return;
  event.preventDefault();
  top.close();
}

/** Escape closes an open panel or drawer, the last one opened first. */
export function useEscape(open: boolean, close: () => void) {
  const latest = useRef(close);
  useEffect(() => {
    latest.current = close;
  });

  useEffect(() => {
    if (!open) return;
    const entry = { close: () => latest.current() };
    openPanels.push(entry);
    if (openPanels.length === 1) window.addEventListener("keydown", onKey);
    return () => {
      const index = openPanels.indexOf(entry);
      if (index >= 0) openPanels.splice(index, 1);
      if (openPanels.length === 0) window.removeEventListener("keydown", onKey);
    };
  }, [open]);
}
