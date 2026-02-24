# WhCatMeme

A production-focused Node native addon built with `napi-rs`, powered by Rust `meme_generator`.

## Highlights

- Full meme-generator bridge for Node.js (JS/TS friendly)
- Dynamic meme key mapping (500+ built-in templates + extensible)
- Per-meme enable/disable with SQLite persistence
- Preflight validation before generation
- Structured resource status checks
- Standardized machine-readable error codes
- Panic-safe Rust bridge to protect Node process stability

## Installation

```bash
npm install WhCatMeme
```

Or with yarn:

```bash
yarn add WhCatMeme
```

## Quick Start

```ts
import { readFileSync, writeFileSync } from 'node:fs'
import { MemeGenerator } from 'WhCatMeme'

const meme = new MemeGenerator({
  dbPath: './data/whcatmeme.sqlite',
  maxTextLength: 512,
})

const payload = {
  key: 'petpet',
  images: [{ data: readFileSync('./avatar.png') }],
  texts: ['hello'],
  options: { circle: true },
}

const preflight = meme.validateGeneratePayload(payload)
if (!preflight.ok) {
  console.error(preflight.issues)
  process.exit(1)
}

const result = meme.generateMemeDetailed(payload)
writeFileSync('./out.gif', result.buffer)
console.log(result.mime, result.usedImages, result.usedTexts)
```

## Documentation

- Full API (English): [API_EN.md](./API_EN.md)
- 完整 API（中文）: [API_ZH.md](./API_ZH.md)
- 中文说明文档: [README.zh-CN.md](./README.zh-CN.md)

## Core APIs

- `validateGeneratePayload(payload)`
- `generateMeme(payload)` / `generateMemeDetailed(payload)`
- `generateMemePreview(key, options?)`
- `getResourceStatus(key?)`
- `generateRandom(payload?)`
- `setMemeEnabled(key, enabled)` / `isMemeEnabled(key)`

## Local Development

Requirements:

- Rust toolchain (stable)
- Node.js (LTS recommended)
- Yarn or npm

Build:

```bash
yarn
yarn build
```

Type check Rust side:

```bash
cargo check
```

## Notes

- This project prioritizes runtime safety in mixed Node/Rust process model.
- Input validation is strict by design to reduce integration risk.
- If you use external meme packs/resources, run resource checks before production traffic.
- Prebuilt binaries remove Rust compile requirements, but image/font assets are not fully bundled by default.  
  Call `checkResources()` or `checkResourcesInBackground()` at startup, or pre-provision assets under `MEME_HOME`.

## Acknowledgements

Special thanks to [`meme_generator`](https://github.com/MemeCrafters/meme-generator-rs), the core meme generation engine used by this project.

## Support

If this project helps you, please consider giving it a Star.
