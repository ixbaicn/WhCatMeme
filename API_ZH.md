# WhCatMeme API 使用文档（完整）

本文档对应当前 `MemeGenerator` 全量接口。

## 1. 初始化

### `new MemeGenerator(options?)`

参数：
- `options.dbPath?: string`：SQLite 状态库路径（选填）
- `options.maxTextLength?: number`：单条文本最大长度，范围 `[1, 4096]`（选填，默认 `512`）

示例：
```ts
import { MemeGenerator } from './index'

const meme = new MemeGenerator({
  dbPath: './data/whcatmeme.sqlite',
  maxTextLength: 512,
})
```

## 2. 元信息接口

### `version(): string`
返回底层 `meme_generator` 版本号。

### `memeHome(): string`
返回 `MEME_HOME` 路径。

### `stateDbPath(): string`
返回启用/禁用 SQLite 路径。

### `readConfigFile(): string`
返回底层配置文件文本（`config.toml`）。

## 3. Meme 查询接口

### `getMemeKeys(includeDisabled?: boolean): string[]`
- `includeDisabled` 选填，默认 `false`

### `getMemeInfo(key: string): MemeInfoDto | null`
- `key` 必填
- 如果 key 不存在或被禁用，返回 `null`

### `getMemesInfo(includeDisabled?: boolean): MemeInfoDto[]`
返回强类型 `MemeInfoDto` 列表。

### `searchMemes(query: string, includeTags?: boolean, includeDisabled?: boolean): string[]`
- `query` 必填
- `includeTags` 选填，默认 `true`
- `includeDisabled` 选填，默认 `false`

## 4. 启用/禁用（持久化）

### `setMemeEnabled(key: string, enabled: boolean): void`
- key 不存在会抛错

### `isMemeEnabled(key: string): boolean`

### `listMemeStates(): MemeState[]`

### `getDisabledMemeKeys(): string[]`

## 5. 生成接口

### `generateMeme(payload: GenerateMemePayload): Buffer`
标准生成接口。

### `generateMemeDetailed(payload: GenerateMemePayload): GenerateMemeResult`
返回结构：
- `buffer: Buffer`
- `mime: string`（如 `image/png` / `image/gif`）
- `usedImages: number`
- `usedTexts: number`
- `key: string`

### `generateMemePreview(key: string, options?): Buffer`
生成模板预览图。

## 6. 生成前预检（新增）

### `validateGeneratePayload(payload: GenerateMemePayload): GenerateValidationResult`
用于“可生成性预检”，不需要靠 `generateMeme` 抛错试探。

返回：
- `ok: boolean`
- `issues: ValidationIssue[]`
- `requiredMinImages / requiredMaxImages / requiredMinTexts / requiredMaxTexts`

`ValidationIssue` 结构：
- `code: string`
- `field: string`
- `message: string`

典型错误码：
- `IMAGE_COUNT_MISMATCH`
- `TEXT_COUNT_MISMATCH`
- `RESOURCE_MISSING`
- `INVALID_OPTION`
- `MEME_NOT_FOUND`
- `MEME_DISABLED`
- `INVALID_PAYLOAD`

## 7. 资源状态（新增）

### `getResourceStatus(key?: string): ResourceStatus[]`
返回“资源是否可用”的结构化结果，减少随机失败。

`ResourceStatus`：
- `key: string`
- `enabled: boolean`
- `available: boolean`
- `code?: string`
- `message?: string`

当资源缺失时，通常会出现：
- `code = RESOURCE_MISSING`

部署说明：
- 使用预编译 `.node` 二进制可以跳过本地 Rust 编译，但运行时仍需要图片/字体素材资源。
- 建议在启动时调用 `checkResources()` / `checkResourcesInBackground()`，或在部署阶段预置 `MEME_HOME` 资源目录。

## 8. 随机生成（新增）

### `generateRandom(payload?: GenerateRandomPayload): GenerateMemeResult`
内置随机生成，支持过滤条件和智能类型筛选。

`payload.filters` 支持：
- `requireImages?: boolean`
- `minTexts?: number`
- `maxTexts?: number`
- `excludeKeys?: string[]`
- `includeDisabled?: boolean`
- `preferType?: 'image' | 'text' | 'any'` — 偏好类型筛选：
  - `'image'`：只从图片型表情包（max_images > 0）中随机
  - `'text'`：只从文本型表情包（max_texts > 0）中随机
  - `'any'`：不区分类型
  - 不传时根据输入自动推断（有图片→image，有文本→text，都有/都没有→any）
- `fallbackPreview?: boolean` — 是否允许预览兜底（默认 `true`）
  - 当所有候选生成失败时，回退到随机生成一个表情包预览图
  - 设为 `false` 可禁用此行为，保持原有的报错逻辑

`payload` 同时可带：
- `images?: InputImagePayload[]`
- `texts?: string[]`
- `options?: Record<string, boolean | string | number>`

改进说明：
- 支持通过 `default_texts` 自动补充缺失文本，大幅增加匹配成功率
- 传入图片时优先匹配图片型表情包，传入文本时优先匹配文本型表情包
- 不传任何内容时直接走预览兜底，返回随机表情包预览图
- 当严格匹配无候选时，会逐步放宽条件（三级回退策略）
- 默认启用预览兜底，确保 `generateRandom` 几乎不会报错
- 返回结果新增 `fallback` 字段，标记是否为兜底预览结果

## 9. 图像工具接口

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

约束：
- 单个 `Buffer` 不能为空
- 单个 `Buffer` 最大 20MB
- 多图接口最多 32 张

## 10. 列表图与统计图

### `renderMemeList(params?): Buffer`
渲染 meme 列表图。

### `renderMemeStatistics(params): Buffer`
渲染统计图。

## 11. 强类型重点（前端表单可直接用）

`getMemeInfo().params` 现在为强类型：
- `minImages / maxImages`
- `minTexts / maxTexts`
- `defaultTexts`
- `options: MemeOptionDto[]`

每个 `MemeOptionDto` 包含：
- `optionType`（boolean/string/integer/float）
- `name`
- `defaultValue`
- `choices`（字符串枚举时）
- `minimum/maximum`
- `parserFlags`

## 12. 错误码标准化

生成链路错误会带机器可读 code（错误文本前缀形如 `[CODE] ...`），便于业务层稳定处理。

常见：
- `IMAGE_COUNT_MISMATCH`
- `TEXT_COUNT_MISMATCH`
- `ASSET_MISSING`
- `RESOURCE_MISSING`
- `INVALID_OPTION`
- `MEME_NOT_FOUND`
- `MEME_DISABLED`
- `RANDOM_GENERATION_FAILED`
- `INTERNAL_PANIC`

## 13. 示例（建议接入方式）

```ts
import { readFileSync } from 'node:fs'
import { MemeGenerator } from './index'

const meme = new MemeGenerator()

const payload = {
  key: 'petpet',
  images: [{ data: readFileSync('./avatar.png') }],
  texts: ['你好'],
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
