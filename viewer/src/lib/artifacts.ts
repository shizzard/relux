import type { ArtifactEntry } from '../types/ArtifactEntry';

/// Encode a forward-slash artifact path for use as a relative URL fragment.
/// Each path segment is `encodeURIComponent`'d independently and re-joined
/// with literal `/`, so directory boundaries stay addressable while spaces,
/// unicode, and special characters survive.
export function encodeArtifactPath(path: string): string {
  return path.split('/').map(encodeURIComponent).join('/');
}

/// Substring filter (case-insensitive) on artifact paths.
export function filterArtifacts(rows: ArtifactEntry[], query: string): ArtifactEntry[] {
  const trimmed = query.trim().toLowerCase();
  if (trimmed.length === 0) return rows;
  return rows.filter((r) => r.path.toLowerCase().includes(trimmed));
}
