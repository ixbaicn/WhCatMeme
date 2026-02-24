# WhCatMeme

[![CI](https://github.com/ixbaicn/WhCatMeme/actions/workflows/CI.yml/badge.svg)](https://github.com/ixbaicn/WhCatMeme/actions/workflows/CI.yml)

`WhCatMeme` 是一个面向生产的 Node 原生扩展，基于 `napi-rs` 构建，底层引擎为 Rust `meme_generator`。

## 项目特性

- 完整映射 meme-generator 到 Node.js（JS/TS 友好）
- 动态 meme key 映射（500+ 内置模板，可扩展）
- 每个 meme 可独立启用/禁用（SQLite 持久化）
- 生成前预检（避免靠异常试探）
- 资源可用性状态查询
- 机器可读错误码标准化
- Rust `panic` 隔离，保护 Node 进程稳定性

## 从源码构建

当前项目以源码方式使用（尚未发布到 npm）。

```bash
git clone https://github.com/ixbaicn/WhCatMeme.git
cd WhCatMeme
yarn install
yarn build
```

构建完成后，可直接通过本地 `index.js` / `index.d.ts` 入口使用扩展。

## 快速开始

```ts
import { readFileSync, writeFileSync } from 'node:fs'
import { MemeGenerator } from './index'

const meme = new MemeGenerator({
  dbPath: './data/whcatmeme.sqlite',
  maxTextLength: 512,
})

const payload = {
  key: 'petpet',
  images: [{ data: readFileSync('./avatar.png') }],
  texts: ['你好'],
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

## 文档

- API 文档（英文）: [API_EN.md](./API_EN.md)
- API 文档（中文）: [API_ZH.md](./API_ZH.md)
- README（English）: [README.md](./README.md)

## 核心接口

- `validateGeneratePayload(payload)`
- `generateMeme(payload)` / `generateMemeDetailed(payload)`
- `generateMemePreview(key, options?)`
- `getResourceStatus(key?)`
- `generateRandom(payload?)`
- `setMemeEnabled(key, enabled)` / `isMemeEnabled(key)`

## 本地开发

环境要求：

- Rust stable
- Node.js（建议 LTS）
- Yarn 或 npm

构建：

```bash
yarn
yarn build
```

Rust 编译检查：

```bash
cargo check
```

## 说明

- 本项目优先保证 Node + Rust 同进程场景下的稳定性。
- 输入校验默认较严格，用于降低业务接入风险。
- 使用外部 meme 包或资源时，建议先执行资源检查再接入线上流量。
- 使用预编译二进制可以免去 Rust 编译，但默认不会完整内置图片/字体素材。  
  建议在启动时调用 `checkResources()` 或 `checkResourcesInBackground()`，或在部署阶段预置 `MEME_HOME` 资源目录。

## 鸣谢

特别鸣谢 [`meme_generator`](https://github.com/MemeCrafters/meme-generator-rs)，本项目的核心表情包生成引擎。

## 支持项目

如果这个项目对你有帮助，欢迎点一个 Star 支持。
