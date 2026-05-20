// Best-effort copy: prefer the async clipboard API, fall back to a hidden
// textarea + `execCommand('copy')` when the API is unavailable (some
// browsers/contexts gate it behind a permission or require a secure origin).
export async function copy(value: string): Promise<void> {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(value);
      return;
    }
  } catch {
    // fall through to legacy path
  }
  const el = document.createElement('textarea');
  el.value = value;
  el.style.position = 'fixed';
  el.style.opacity = '0';
  document.body.appendChild(el);
  el.focus();
  el.select();
  try {
    document.execCommand('copy');
  } catch {
    // best-effort
  }
  document.body.removeChild(el);
}
