import type { Span } from '../types/Span';

const TRANSPARENT_IMPURE_BIF_NAMES = new Set(['annotate', 'log', 'sleep']);

export function isTransparentBif(span: Span): boolean {
  if (span.kind !== 'fn-call') return false;
  if (span.callee_kind !== 'bif') return false;
  if (span.is_pure) return true;
  return TRANSPARENT_IMPURE_BIF_NAMES.has(span.name);
}
