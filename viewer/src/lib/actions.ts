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
