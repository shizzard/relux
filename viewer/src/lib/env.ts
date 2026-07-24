import type { EnvValue } from '../types/EnvValue';

/// A provenance tier in the env modal, in display order:
///   1. `relux`  - values Relux injects for the run (highest precedence)
///   2. `dotenv` - values from a committed `.env` file (one section per file)
///   3. `host`   - values inherited from the host process environment
///   4. `other`  - defensive catch-all; sources that cannot normally reach the
///                 bootstrap snapshot (effect overlays). Never expected, but
///                 surfaced rather than dropped.
export type EnvTier = 'relux' | 'dotenv' | 'host' | 'other';

/// One rendered section of the env modal: a labelled, ordered run of rows.
export interface EnvSection {
  /// Stable key for `{#each}`; unique across sections.
  id: string;
  tier: EnvTier;
  /// Human label shown in the section header.
  label: string;
  /// Full source path, present only for `dotenv` sections (used as a tooltip
  /// so the suite-relative `label` can stay short).
  path?: string;
  rows: EnvValue[];
}

/// The suite root Relux records for the run, used to shorten `.env` labels.
/// Read from the bootstrap dump itself (`__RELUX_SUITE_ROOT`).
export function findSuiteRoot(rows: EnvValue[]): string | null {
  const row = rows.find((r) => r.key === '__RELUX_SUITE_ROOT');
  return row ? row.value : null;
}

/// Split a path into its non-empty components, tolerating both `/` and `\`.
function components(path: string): string[] {
  return path.split(/[/\\]+/).filter((c) => c.length > 0);
}

/// The directory depth of a `.env` file: how many path components precede the
/// file name. Deeper files load later and win, so a larger depth means higher
/// precedence.
function depth(path: string): number {
  return Math.max(components(path).length - 1, 0);
}

/// Label a `.env` file relative to the suite root as `.../<relative path>`, so
/// nested files read as `.../.env`, `.../tests/deep/.env`. Falls back to the
/// full path when the suite root is unknown or the file sits outside it.
function labelFor(path: string, suiteRoot: string | null): string {
  if (!suiteRoot) return path;
  const root = components(suiteRoot);
  const full = components(path);
  for (let i = 0; i < root.length; i++) {
    if (full[i] !== root[i]) return path;
  }
  const rel = full.slice(root.length).join('/');
  return rel.length > 0 ? `.../${rel}` : path;
}

/// Build the ordered env-modal sections from the bootstrap rows.
///
/// Rows keep their incoming order within each section (the runtime emits them
/// key-sorted). `.env` rows are split into one section per source file, ordered
/// deepest-first so the highest-precedence file sits nearest the top of the
/// tier, and labelled relative to `suiteRoot`. Empty sections are omitted.
export function buildEnvSections(rows: EnvValue[], suiteRoot: string | null): EnvSection[] {
  const relux: EnvValue[] = [];
  const host: EnvValue[] = [];
  const other: EnvValue[] = [];
  const byFile = new Map<string, EnvValue[]>();

  for (const row of rows) {
    switch (row.source.kind) {
      case 'relux-internal':
        relux.push(row);
        break;
      case 'base':
        host.push(row);
        break;
      case 'dot-env': {
        let bucket = byFile.get(row.source.path);
        if (!bucket) {
          bucket = [];
          byFile.set(row.source.path, bucket);
        }
        bucket.push(row);
        break;
      }
      default:
        other.push(row);
        break;
    }
  }

  const sections: EnvSection[] = [];

  if (relux.length > 0) {
    sections.push({ id: 'relux', tier: 'relux', label: 'relux internals', rows: relux });
  }

  const dotenv = [...byFile.keys()]
    .sort((a, b) => depth(b) - depth(a) || a.localeCompare(b))
    .map((path) => ({
      id: `dotenv:${path}`,
      tier: 'dotenv' as const,
      label: labelFor(path, suiteRoot),
      path,
      rows: byFile.get(path)!,
    }));
  sections.push(...dotenv);

  if (host.length > 0) {
    sections.push({ id: 'host', tier: 'host', label: 'host environment', rows: host });
  }
  if (other.length > 0) {
    sections.push({ id: 'other', tier: 'other', label: 'other', rows: other });
  }

  return sections;
}
