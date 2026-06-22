# WhCatMeme API Documentation (Complete)

This document covers all currently exported `MemeGenerator` APIs.

## 1. Initialization

### `new MemeGenerator(options?)`

Parameters:
- `options.dbPath?: string`: SQLite state database path (optional)
- `options.maxTextLength?: number`: max length for each text item, range `[1, 4096]` (optional, default `512`)

Example:
```ts
import { MemeGenerator } from './index'

const meme = new MemeGenerator({
  dbPath: './data/whcatmeme.sqlite',
  maxTextLength: 512,
})
```

## 2. Metadata APIs

### `version(): string`
Returns the underlying `meme_generator` version.

### `memeHome(): string`
Returns the `MEME_HOME` path.

### `stateDbPath(): string`
Returns the SQLite path used for enable/disable state.

### `readConfigFile(): string`
Returns raw `config.toml` content.

## 3. Meme Discovery APIs

### `getMemeKeys(includeDisabled?: boolean): string[]`
- `includeDisabled` optional, default `false`

### `getMemeInfo(key: string): MemeInfoDto | null`
- `key` required
- returns `null` when key does not exist or is disabled

### `getMemesInfo(includeDisabled?: boolean): MemeInfoDto[]`
Returns strongly typed meme info list.

### `searchMemes(query: string, includeTags?: boolean, includeDisabled?: boolean): string[]`
- `query` required
- `includeTags` optional, default `true`
- `includeDisabled` optional, default `false`

## 4. Enable/Disable State (Persisted)

### `setMemeEnabled(key: string, enabled: boolean): void`
- throws when key does not exist

### `isMemeEnabled(key: string): boolean`

### `listMemeStates(): MemeState[]`

### `getDisabledMemeKeys(): string[]`

## 5. Generation APIs

### `generateMeme(payload: GenerateMemePayload): Buffer`
Standard generation API.

### `generateMemeDetailed(payload: GenerateMemePayload): GenerateMemeResult`
Returns:
- `buffer: Buffer`
- `mime: string` (e.g. `image/png` / `image/gif`)
- `usedImages: number`
- `usedTexts: number`
- `key: string`

### `generateMemePreview(key: string, options?): Buffer`
Generates preview output for a template.

## 6. Preflight Validation (New)

### `validateGeneratePayload(payload: GenerateMemePayload): GenerateValidationResult`
Checks if payload is generatable without trial-and-error execution.

Returns:
- `ok: boolean`
- `issues: ValidationIssue[]`
- `requiredMinImages / requiredMaxImages / requiredMinTexts / requiredMaxTexts`

`ValidationIssue`:
- `code: string`
- `field: string`
- `message: string`

Typical codes:
- `IMAGE_COUNT_MISMATCH`
- `TEXT_COUNT_MISMATCH`
- `RESOURCE_MISSING`
- `INVALID_OPTION`
- `MEME_NOT_FOUND`
- `MEME_DISABLED`
- `INVALID_PAYLOAD`

## 7. Resource Status (New)

### `getResourceStatus(key?: string): ResourceStatus[]`
Returns structured resource availability result.

`ResourceStatus`:
- `key: string`
- `enabled: boolean`
- `available: boolean`
- `code?: string`
- `message?: string`

When resources are missing, expected code is usually:
- `RESOURCE_MISSING`

Deployment note:
- Using prebuilt `.node` binaries skips local Rust compilation, but image/font assets are still required at runtime.
- Run `checkResources()` / `checkResourcesInBackground()` on startup, or pre-provision resources under `MEME_HOME`.

## 8. Random Generation (New)

### `generateRandom(payload?: GenerateRandomPayload): GenerateMemeResult`
Built-in random generation with filters and smart type-based selection.

`payload.filters`:
- `requireImages?: boolean`
- `minTexts?: number`
- `maxTexts?: number`
- `excludeKeys?: string[]`
- `includeDisabled?: boolean`
- `preferType?: 'image' | 'text' | 'any'` — Preference type filter:
  - `'image'`: only randomize from image-type memes (max_images > 0)
  - `'text'`: only randomize from text-type memes (max_texts > 0)
  - `'any'`: no type filtering
  - When omitted, auto-inferred from input (images→image, texts→text, both/none→any)
- `fallbackPreview?: boolean` — Whether to allow preview fallback when all candidates fail (default: `true`)
  - Falls back to generating a random meme preview when all candidates fail
  - Set to `false` to disable fallback and preserve the original error behavior

`payload` can also include:
- `images?: InputImagePayload[]`
- `texts?: string[]`
- `options?: Record<string, boolean | string | number>`

Improvements:
- Supports auto-supplementing missing texts via `default_texts`, greatly increasing match success rate
- Prioritizes image-type memes when images are provided, text-type memes when texts are provided
- When no images or texts are provided, goes directly to fallback preview
- Gradually relaxes matching criteria when strict matching yields no candidates (3-level fallback strategy)
- Preview fallback enabled by default, ensuring `generateRandom` almost never throws errors
- Result includes a `fallback` field indicating whether the result is a fallback preview

## 9. Image Tool APIs

- `inspectImage(image)`
- `flipHorizontal(image)`
- `flipVertical(image)`
- `rotate(image, degrees?)`
- `resize(image, width?, height?)`
- `crop(image, left?, top?, right?, bottom?)`
- `grayscale(image)`
- `invert(image)`
- `mergeHorizontal(images)`
- `mergeVertical(images)`
- `gifSplit(image)`
- `gifMerge(images, duration?)`
- `gifReverse(image)`
- `gifChangeDuration(image, duration)`

Constraints:
- each `Buffer` must be non-empty
- max size per `Buffer` is 20MB
- max image count for multi-image APIs is 32

## 10. List/Stats Rendering

### `renderMemeList(params?): Buffer`
Renders meme list image.

### `renderMemeStatistics(params): Buffer`
Renders statistics chart image.

## 11. Strong Types for Dynamic UI

`getMemeInfo().params` is now strongly typed:
- `minImages / maxImages`
- `minTexts / maxTexts`
- `defaultTexts`
- `options: MemeOptionDto[]`

Each `MemeOptionDto` includes:
- `optionType` (`boolean` / `string` / `integer` / `float`)
- `name`
- `defaultValue`
- `choices` (for string enums)
- `minimum / maximum`
- `parserFlags`

## 12. Standardized Error Codes

Generation pipeline errors now carry machine-readable code (`[CODE] message`).

Common codes:
- `IMAGE_COUNT_MISMATCH`
- `TEXT_COUNT_MISMATCH`
- `ASSET_MISSING`
- `RESOURCE_MISSING`
- `INVALID_OPTION`
- `MEME_NOT_FOUND`
- `MEME_DISABLED`
- `RANDOM_GENERATION_FAILED`
- `INTERNAL_PANIC`

## 13. Recommended Integration Example

```ts
import { readFileSync } from 'node:fs'
import { MemeGenerator } from './index'

const meme = new MemeGenerator()

const payload = {
  key: 'petpet',
  images: [{ data: readFileSync('./avatar.png') }],
  texts: ['hello'],
  options: { circle: true },
}

const check = meme.validateGeneratePayload(payload)
if (!check.ok) {
  console.error(check.issues)
  process.exit(1)
}

const result = meme.generateMemeDetailed(payload)
console.log(result.mime, result.usedImages, result.usedTexts)
```
