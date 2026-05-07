import { readFile, writeFile } from 'node:fs/promises';
import { gzipSync } from 'node:zlib';

const src = 'dist/relux-viewer.js';
const dst = 'dist/relux-viewer.js.gz';

const input = await readFile(src);
const compressed = gzipSync(input, { level: 9 });
await writeFile(dst, compressed);

const ratio = ((compressed.length / input.length) * 100).toFixed(1);
console.log(`gzipped ${src} (${input.length} B) -> ${dst} (${compressed.length} B, ${ratio}%)`);
