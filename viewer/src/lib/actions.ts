// Svelte action: fire `onOutside` when a click (bubble phase) lands outside
// the node. Registered on the next tick so the click that mounted the
// popup doesn't immediately close it. Use `ignore` to exclude a sibling
// element (e.g. the toggle button that owns the popup) so its onclick can
// drive the toggle without the outside-handler racing it.
export function clickOutside(
  node: HTMLElement,
  onOutside: () => void,
): { destroy(): void } {
  const handler = (event: MouseEvent): void => {
    if (node.contains(event.target as Node)) return;
    onOutside();
  };
  const id = window.setTimeout(() => window.addEventListener('click', handler), 0);
  return {
    destroy() {
      window.clearTimeout(id);
      window.removeEventListener('click', handler);
    },
  };
}

// Svelte action: probe a node for horizontal overflow and report changes
// via a callback. The callback fires immediately on attach, then again
// whenever the node's content/box geometry changes (ResizeObserver) so
// callers don't need to plumb resize events manually.
export function overflowProbe(
  node: HTMLElement,
  callback: (overflowing: boolean) => void,
): { destroy(): void } {
  let last = !node.isConnected;
  const check = (): void => {
    const overflowing = node.scrollWidth > node.clientWidth;
    if (overflowing !== last) {
      last = overflowing;
      callback(overflowing);
    }
  };
  check();
  const ro = new ResizeObserver(check);
  ro.observe(node);
  return {
    destroy() {
      ro.disconnect();
    },
  };
}
