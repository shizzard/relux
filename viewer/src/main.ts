import { mount } from 'svelte';
import App from './App.svelte';
import type { StructuredLog } from './types/StructuredLog';

declare global {
  interface Window {
    RELUX_DATA?: StructuredLog;
  }
}

mount(App, {
  target: document.getElementById('app')!,
  props: { data: window.RELUX_DATA ?? null },
});
