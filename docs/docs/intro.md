---
sidebar_position: 1
---

# Introduction

**ShojiWM** is a programmable Wayland compositor. Window decorations,
layout, and visual effects are described in TypeScript/TSX by default (or in
Rust, or any language you plug in), while the compositor core is written in
Rust on top of [Smithay](https://github.com/Smithay/smithay).

This documentation is a work in progress. The pages below are placeholders
and will be filled in over time.

## Highlights

- **Declarative composition** — describe your window chrome and layout with a
  React-like TSX API.
- **Reactive signals** — UI updates automatically when state changes.
- **GPU effects** — blur, shaders, and per-window transforms.
- **Hot reload** — iterate on your TypeScript config without restarting the session
  (`Super` + `Shift` + `R`).
- **Any config language** — TypeScript/TSX out of the box, Rust through
  `shojiwm_rs` (the default config is ported as an example), and a
  language-neutral runtime interface for anything else. See
  [Config languages](./configuration/languages.md).

## Where to next

- [Getting Started](./getting-started/installation.md) — install and run ShojiWM.
- [Configuration](./configuration/overview.md) — learn how the config layer works.
