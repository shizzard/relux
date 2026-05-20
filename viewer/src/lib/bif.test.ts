import { describe, expect, it } from 'vitest';
import type { Span } from '../types/Span';
import { isTransparentBif } from './bif';

function fnCallSpan(overrides: {
  name?: string;
  callee_kind?: 'user' | 'bif';
  is_pure?: boolean;
}): Span {
  return {
    id: 1n,
    parent: null,
    start_ts: 0,
    end_ts: null,
    location: null,
    kind: 'fn-call',
    name: overrides.name ?? 'f',
    args: [],
    result: null,
    callee_kind: overrides.callee_kind ?? 'user',
    is_pure: overrides.is_pure ?? false,
  };
}

describe('isTransparentBif', () => {
  it('returns false for user functions', () => {
    expect(isTransparentBif(fnCallSpan({ callee_kind: 'user', name: 'my_helper' }))).toBe(false);
    expect(
      isTransparentBif(fnCallSpan({ callee_kind: 'user', is_pure: true, name: 'pure_helper' })),
    ).toBe(false);
  });

  it('returns true for pure BIFs', () => {
    expect(isTransparentBif(fnCallSpan({ callee_kind: 'bif', is_pure: true, name: 'trim' }))).toBe(
      true,
    );
    expect(isTransparentBif(fnCallSpan({ callee_kind: 'bif', is_pure: true, name: 'rand' }))).toBe(
      true,
    );
  });

  it('returns true for transparent impure BIFs', () => {
    expect(isTransparentBif(fnCallSpan({ callee_kind: 'bif', name: 'annotate' }))).toBe(true);
    expect(isTransparentBif(fnCallSpan({ callee_kind: 'bif', name: 'log' }))).toBe(true);
    expect(isTransparentBif(fnCallSpan({ callee_kind: 'bif', name: 'sleep' }))).toBe(true);
  });

  it('returns false for kept-wrapper impure BIFs', () => {
    expect(isTransparentBif(fnCallSpan({ callee_kind: 'bif', name: 'match_ok' }))).toBe(false);
    expect(isTransparentBif(fnCallSpan({ callee_kind: 'bif', name: 'match_not_ok' }))).toBe(false);
    expect(isTransparentBif(fnCallSpan({ callee_kind: 'bif', name: 'ctrl_c' }))).toBe(false);
    expect(isTransparentBif(fnCallSpan({ callee_kind: 'bif', name: 'ctrl_backslash' }))).toBe(
      false,
    );
  });

  it('returns false for non-fn-call spans', () => {
    const testSpan: Span = {
      id: 1n,
      parent: null,
      start_ts: 0,
      end_ts: null,
      location: null,
      kind: 'test',
      name: 't',
    };
    expect(isTransparentBif(testSpan)).toBe(false);
  });
});
