---
sidebar_position: 1
---

# はじめに

**ShojiWM** はプログラマブルな Wayland コンポジターです。ウィンドウの装飾・
レイアウト・視覚エフェクトは標準では TypeScript/TSX で記述し（Rust や、自分で組み込んだ
任意の言語でも書けます）、コンポジターのコアは
[Smithay](https://github.com/Smithay/smithay) を基盤とした Rust で実装されています。

## 特徴

- **宣言的なコンポジション** — React 風の TSX API でウィンドウのレイアウトを記述します。

```tsx
COMPOSITOR.window.composition = (window: WaylandWindow) => (
  <ManagedWindow rect={window.position} zIndex={1}>
    <WindowBorder style={{ border: { px: 2, color: "#FFFFFF" }, borderRadius: 8 }}>
      <ClientWindow />
    </WindowBorder>
  </ManagedWindow>
)
```

- **リアクティブなシグナル** — 状態が変化すると UI が自動的に更新されます。

```tsx
const CloseButton = ({ window }: { window: WaylandWindow }) => {
  const [hover, setHover] = useState(false)

  const borderColor = hover((hover) => (hover ? "#00000000" : "#F0808030"))

  var icon: CompositionRenderable | null = null
  if (hover()) {
    icon = (
      <Image
        src="./assets/x.svg"
        style={{
          width: 16,
          height: 16,
          position: "absolute",
          zIndex: 1,
          pointerEvents: "none",
        }}
      />
    )
  }

  return (
    <Box style={{ position: "relative", flexShrink: 0 }}>
      <Button
        onHoverChange={setHover}
        style={{
          width: 16,
          height: 16,
          borderRadius: 8,
          background: "#FFFFFF20",
          border: { px: 1, color: borderColor },
        }}
        onClick={window.close}
      />
      {icon}
    </Box>
  )
}
```

- **GPU エフェクト** — ブラー、カスタムシェーダー

```tsx
const LAYER_BLUR_MASK = compileLayerEffect({
  input: backdropSource(),
  capturePadding: 24,
  invalidate: { kind: "on-source-damage-box", damagePadding: 8 },
  alpha: "preserve",
  pipeline: [
    dualKawaseBlur({ radius: 4, passes: 2 }),
    shaderStage(loadShader("./src/layer-blur-mask.frag"), {
      textures: {
        layer_mask: layerSource(),
      },
      uniforms: {
        opacity_threshold: 0.25,
        mask_feather: 0.04,
      },
    }),
  ],
})
```

- **ホットリロード** — セッションを再起動せずに TypeScript の設定を反復開発できます
  （`Super` + `Shift` + `R`）。
- **任意の設定言語** — TypeScript/TSX は標準で、Rust は `shojiwm_rs` で使えます
  （デフォルト設定の Rust 移植が example として付属）。それ以外の言語も、言語に
  依存しないランタイムインターフェースで組み込めます。詳しくは
  [設定言語](./configuration/languages.md) を参照してください。

## 次に読むべきページ

- [はじめかた](./getting-started/installation.md) — ShojiWM をインストールして起動します。
- [アーキテクチャ](./architecture/wayland.md) — ShojiWM や Wayland の仕組みを学びます。
- [設定](./configuration/overview.md) — 設定レイヤーの仕組みを学びます。
