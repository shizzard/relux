import tokensCss from './styles/tokens.css?inline';
import globalCss from './styles/global.css?inline';
import { mount } from 'svelte';
import App from './App.svelte';
import type { StructuredLog } from './types/StructuredLog';

declare global {
  interface Window {
    RELUX_DATA?: StructuredLog;
  }
}

const style = document.createElement('style');
style.textContent = `${tokensCss}\n${globalCss}`;
document.head.appendChild(style);

mount(App, {
  target: document.getElementById('app')!,
  props: { data: window.RELUX_DATA ?? null },
});
