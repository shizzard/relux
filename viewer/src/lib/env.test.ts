import { describe, expect, it } from 'vitest';
import type { EnvValue } from '../types/EnvValue';
import type { EnvSourceRecord } from '../types/EnvSourceRecord';
import { buildEnvSections, findSuiteRoot } from './env';

const row = (key: string, source: EnvSourceRecord): EnvValue => ({
  key,
  value: `${key}-value`,
  source,
});

const base = (key: string) => row(key, { kind: 'base' });
const relux = (key: string) => row(key, { kind: 'relux-internal' });
const dot = (key: string, path: string) => row(key, { kind: 'dot-env', path });
const suiteRootRow = (value: string): EnvValue => ({
  key: '__RELUX_SUITE_ROOT',
  value,
  source: { kind: 'relux-internal' },
});

describe('findSuiteRoot', () => {
  it('reads __RELUX_SUITE_ROOT from the rows', () => {
    expect(findSuiteRoot([base('HOME'), suiteRootRow('/proj')])).toBe('/proj');
  });

  it('returns null when absent', () => {
    expect(findSuiteRoot([base('HOME')])).toBeNull();
  });
});

describe('buildEnvSections', () => {
  it('orders tiers relux, then dotenv, then host', () => {
    const sections = buildEnvSections(
      [base('HOME'), dot('SHARED', '/proj/.env'), relux('__RELUX_RUN')],
      '/proj',
    );
    expect(sections.map((s) => s.tier)).toEqual(['relux', 'dotenv', 'host']);
  });

  it('omits empty tiers', () => {
    const sections = buildEnvSections([base('HOME'), base('PATH')], '/proj');
    expect(sections).toHaveLength(1);
    expect(sections[0].tier).toBe('host');
    expect(sections[0].rows.map((r) => r.key)).toEqual(['HOME', 'PATH']);
  });

  it('splits dotenv into one section per file, deepest first', () => {
    const sections = buildEnvSections(
      [
        dot('ROOT_ONLY', '/proj/.env'),
        dot('DEEP_ONLY', '/proj/tests/deep/.env'),
        dot('MID', '/proj/tests/.env'),
      ],
      '/proj',
    );
    const dotenv = sections.filter((s) => s.tier === 'dotenv');
    expect(dotenv.map((s) => s.path)).toEqual([
      '/proj/tests/deep/.env',
      '/proj/tests/.env',
      '/proj/.env',
    ]);
  });

  it('labels dotenv files relative to the suite root as .../<path>', () => {
    const sections = buildEnvSections(
      [dot('A', '/proj/.env'), dot('B', '/proj/tests/deep/.env')],
      '/proj',
    );
    const dotenv = sections.filter((s) => s.tier === 'dotenv');
    expect(dotenv.map((s) => s.label)).toEqual(['.../tests/deep/.env', '.../.env']);
    // The full path is retained for the tooltip.
    expect(dotenv.map((s) => s.path)).toEqual(['/proj/tests/deep/.env', '/proj/.env']);
  });

  it('labels a lone dotenv file relative to the suite root too', () => {
    const sections = buildEnvSections([dot('A', '/proj/.env')], '/proj');
    const dotenv = sections.find((s) => s.tier === 'dotenv')!;
    expect(dotenv.label).toBe('.../.env');
  });

  it('falls back to the full path when the suite root is unknown', () => {
    const sections = buildEnvSections([dot('A', '/proj/.env')], null);
    expect(sections.find((s) => s.tier === 'dotenv')!.label).toBe('/proj/.env');
  });

  it('falls back to the full path when the file sits outside the suite root', () => {
    const sections = buildEnvSections([dot('A', '/elsewhere/.env')], '/proj');
    expect(sections.find((s) => s.tier === 'dotenv')!.label).toBe('/elsewhere/.env');
  });

  it('preserves incoming row order within a section', () => {
    const sections = buildEnvSections([relux('AAA'), relux('BBB')], '/proj');
    expect(sections[0].rows.map((r) => r.key)).toEqual(['AAA', 'BBB']);
  });

  it('surfaces unexpected sources in an other tier at the bottom', () => {
    const sections = buildEnvSections(
      [base('HOME'), row('OVERLAY', { kind: 'effect-overlay', mnemonic: 'db' })],
      '/proj',
    );
    expect(sections.map((s) => s.tier)).toEqual(['host', 'other']);
    expect(sections[1].rows.map((r) => r.key)).toEqual(['OVERLAY']);
  });
});
