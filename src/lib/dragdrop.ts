import { listen, TauriEvent } from "@tauri-apps/api/event";

import { isTauri } from "./ipc";

export interface DropHandlers {
  /** A drag came over the window (true) or left it (false). Called when
   * that changes, not for every move of the pointer. */
  onHover: (hovering: boolean) => void;
  onDrop: (paths: string[]) => void;
}

/** What Tauri sends with a drop; `position` is not needed here. */
interface DroppedPayload {
  paths: string[];
}

/** What a browser drop pretends to be; the mock backend ignores the path. */
const MOCK_DROP = "/home/alex/projects/sample-stack";

/** Native file drops from Finder anywhere on the window. */
export function watchFileDrop(handlers: DropHandlers): () => void {
  // Drag-over arrives many times a second while a file hovers; only a
  // change is passed on, so the window does no work for the repeats.
  let hovering = false;
  const hover = (now: boolean) => {
    if (now === hovering) return;
    hovering = now;
    handlers.onHover(now);
  };

  if (isTauri) {
    // The same four events Webview.onDragDropEvent listens to, without
    // importing the window and webview modules it needs (about 16 KB). The
    // app has one webview, so listening on every target hears exactly its drags.
    const pending = Promise.all([
      listen(TauriEvent.DRAG_ENTER, () => hover(true)),
      listen(TauriEvent.DRAG_OVER, () => hover(true)),
      listen(TauriEvent.DRAG_LEAVE, () => hover(false)),
      listen<DroppedPayload>(TauriEvent.DRAG_DROP, (event) => {
        hover(false);
        handlers.onDrop(event.payload.paths);
      }),
    ]);
    return () => void pending.then((unlisteners) => unlisteners.forEach((unlisten) => unlisten()));
  }

  const over = (event: DragEvent) => {
    event.preventDefault();
    hover(true);
  };
  const leave = () => hover(false);
  const drop = (event: DragEvent) => {
    event.preventDefault();
    hover(false);
    handlers.onDrop([MOCK_DROP]);
  };
  window.addEventListener("dragover", over);
  window.addEventListener("dragleave", leave);
  window.addEventListener("drop", drop);
  return () => {
    window.removeEventListener("dragover", over);
    window.removeEventListener("dragleave", leave);
    window.removeEventListener("drop", drop);
  };
}
