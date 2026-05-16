import { describe, expect, it } from 'vitest';
import type { ArtifactEntry } from '../types/ArtifactEntry';
import { encodeArtifactPath, filterArtifacts } from './artifacts';

const make = (path: string): ArtifactEntry => ({
  path,
  size: 1n,
  mime: null,
});

describe('encodeArtifactPath', () => {
  it('passes through plain paths unchanged', () => {
    expect(encodeArtifactPath('out.txt')).toBe('out.txt');
    expect(encodeArtifactPath('sut/error.log')).toBe('sut/error.log');
  });

  it('encodes spaces and reserved characters per segment', () => {
    expect(encodeArtifactPath('has spaces/and+plus.png')).toBe(
      'has%20spaces/and%2Bplus.png',
    );
  });

  it('preserves forward slashes as directory separators', () => {
    const encoded = encodeArtifactPath('a/b c/d.txt');
    expect(encoded.split('/').length).toBe(3);
    expect(encoded).toBe('a/b%20c/d.txt');
  });

  it('encodes unicode', () => {
    expect(encodeArtifactPath('caf\u00e9.txt')).toBe('caf%C3%A9.txt');
  });
});

describe('filterArtifacts', () => {
  const rows = [
    make('out.txt'),
    make('screenshot.png'),
    make('sut/error.log'),
    make('sut/logs/foo.log'),
  ];

  it('returns the full list when the query is blank', () => {
    expect(filterArtifacts(rows, '')).toEqual(rows);
    expect(filterArtifacts(rows, '   ')).toEqual(rows);
  });

  it('matches substrings case-insensitively', () => {
    expect(filterArtifacts(rows, 'LOG').map((r) => r.path)).toEqual([
      'sut/error.log',
      'sut/logs/foo.log',
    ]);
  });

  it('matches across path separators', () => {
    expect(filterArtifacts(rows, 'sut/').map((r) => r.path)).toEqual([
      'sut/error.log',
      'sut/logs/foo.log',
    ]);
  });

  it('returns an empty list when nothing matches', () => {
    expect(filterArtifacts(rows, 'zzz')).toEqual([]);
  });
});
