---
sidebar_position: 8.5
---

# ペイントシェーダー

サーバーサイドデコレーションは、すべてシェーダーで描かれています。ペイントシェーダーを
使うと、その描画をコンポーネント単位で置き換えられます。面倒な部分は ShojiWM が引き受けます。
レイアウトは先に**整数の物理ピクセル**へ解決され（fractional scale、辺ごとの丸め、
ウィンドウのサブピクセル位置、クリップ）、シェーダーは各ピクセルの色を決めるだけです。

```tsx
import {Box, paintShader} from 'shoji_wm';

const gradientBorder = paintShader('./shaders/gradient-border.frag');

<WindowBorder
  paint={gradientBorder}
  style={{border: {px: 1.5, color: '#7aa2f7'}, borderRadius: 12, background: '#1e1e2e'}}
>
  ...
</WindowBorder>
```

どのコンポーネントにも 2 つのペイントスロットがあります。

| prop | 描かれる場所 | 用途 |
| --- | --- | --- |
| `paint` | 組み込みの背景・枠の代わり | グラデーション、模様、独自の枠 |
| `overlay` | コンポーネントの子要素より上 | フォーカスリング、光沢、グロー |

組み込みの背景・枠そのものが同じ契約で書かれたペイントシェーダーなので、独自の
`paint` は組み込みの見た目とまったく同じジオメトリから始められます。

## ピクセル単位: `phy_px`

シェーダーが受け取る値はすべて**物理ピクセル**で、名前にも `_phy_px` が付きます
（`size_phy_px`、`border_phy_px` など）。設定のスタイルはこれまでどおり論理ピクセルです。
1.5 倍では `borderRadius: 12` がシェーダーに `radius_phy_px = 18` として届きます。値は
整数で、ピクセルの中心は `.5` にあるため、ピクセル格子上の辺はぼやけずに描けます。

## `paint_main` の契約

ペイントシェーダーは、関数を 1 つ持つ GLSL ファイルです。

```glsl
vec4 paint_main(PaintContext ctx) {
    // このピクセルの色を、アルファ乗算済みで返す
    return ctx.background * shoji_outer_coverage(ctx);
}
```

戻り値は**乗算済みアルファ**（`rgb * a, a`）です。コンテキストの色はすでに乗算済みなので、
カバレッジを掛けるだけで正しくなります。自分で用意した色は
`shoji_premultiply(vec4(r, g, b, a))` で変換してください。

```glsl
struct PaintContext {
    vec2 frag_phy_px;          // このピクセル（コンポーネントの左上が原点）
    vec2 size_phy_px;          // コンポーネントのボーダーボックス
    vec4 border_phy_px;        // 上・右・下・左
    vec4 radius_phy_px;        // 左上・右上・右下・左下
    vec4 padding_phy_px;       // 上・右・下・左
    vec4 content_rect_phy_px;  // 枠とパディングの内側: x, y, width, height
    vec4 inner_rect_phy_px;    // 枠の内側の縁
    vec4 inner_radius_phy_px;
    bool has_hole;             // <WindowBorder>: 内側はクライアントが見える領域
    vec4 background;           // style.background（乗算済み）
    vec4 border_color;         // style.border の色（乗算済み）
    float scale;               // 論理 1px あたりの物理ピクセル数
};
```

`opacity`、角丸の祖先によるクリップ、ウィンドウ自体のフェードは、`paint_main` が返した
あとに ShojiWM が適用します。

### ヘルパー

| ヘルパー | 結果 |
| --- | --- |
| `shoji_rrect_sdf(p, rect, radius)` | 角丸矩形までの符号付き距離（内側が負） |
| `shoji_coverage(sdf)` | 距離からのピクセルカバレッジ（物理 1px 幅のアンチエイリアス） |
| `shoji_rrect_coverage(p, rect, radius)` | 角丸矩形のカバレッジ |
| `shoji_blurred_rrect_coverage(p, rect, radius, sigma)` | ガウスぼかしした角丸矩形のカバレッジ |
| `shoji_outer_coverage(ctx)` | ボーダーボックスのカバレッジ |
| `shoji_inner_coverage(ctx)` | 枠の内側のカバレッジ |
| `shoji_border_coverage(ctx)` | 枠の帯のカバレッジ |
| `shoji_fill_coverage(ctx)` | 背景を塗る範囲（`<WindowBorder>` ではクライアント部分を除く） |
| `shoji_border_color_at(ctx)` | このピクセルが属する辺の枠色 |
| `shoji_default_paint(ctx)` | 組み込みの背景＋枠 |
| `shoji_over(src, dst)` | `src` を `dst` の上に重ねる（乗算済み） |
| `shoji_premultiply(color)` | 非乗算の色を乗算済みに変換 |

矩形は `(x, y, width, height)`、半径は左上・右上・右下・左下の順です。

## 例

通常の背景の上にグラデーションの枠を描く例:

```glsl
// shaders/gradient-border.frag
vec4 paint_main(PaintContext ctx) {
    vec2 uv = ctx.frag_phy_px / ctx.size_phy_px;
    vec4 from = vec4(0.48, 0.64, 0.97, 1.0);
    vec4 to = vec4(0.95, 0.55, 0.66, 1.0);
    vec4 border = mix(from, to, uv.x * 0.7 + uv.y * 0.3) * shoji_border_coverage(ctx);
    vec4 fill = ctx.background * shoji_fill_coverage(ctx);
    return shoji_over(border, fill);
}
```

コンポーネントの周りにグローを描く例（overlay レイヤー）。`outsets` でコンポーネントの
外側に描ける範囲を論理ピクセルで指定します。

```glsl
// shaders/glow.frag
uniform float strength;

vec4 paint_main(PaintContext ctx) {
    float d = shoji_rrect_sdf(ctx.frag_phy_px, vec4(vec2(0.0), ctx.size_phy_px), ctx.radius_phy_px);
    float glow = d > 0.0 ? strength * exp(-d / (3.0 * ctx.scale)) : 0.0;
    return vec4(0.48, 0.64, 0.97, 1.0) * glow;
}
```

```tsx
// 合成関数の中で
const [hover, setHover] = useState(false);
const glow = paintShader('./shaders/glow.frag', {
  uniforms: {strength: hover((on) => (on ? 0.7 : 0))},
  outsets: 16,
});

<Box overlay={glow} onHoverChange={setHover} style={{borderRadius: 8}} />
```

## uniform とアニメーション

`uniforms` にはエフェクトのステージと同じ値（数値、2〜4 要素の配列、`uniformArray(...)`）を
渡せ、それぞれ signal にできます。uniform だけが変わった場合、ShojiWM はウィンドウを
レイアウトし直さずにそのコンポーネントだけを再描画します。

`time` の uniform は自動では渡しません。ペイントシェーダーが再描画されるのは、ジオメトリか
uniform が変わったときだけです。アニメーションさせるには、signal（例えば
[アニメーション](./animations.md)の値）で uniform を動かしてください。

`shoji_` で始まる名前と `alpha`・`size`・`tex` は予約されています。

## 影: `boxShadow`

CSS と同じ `boxShadow` が組み込まれているので、多くの影はシェーダーなしで描けます。

```tsx
<WindowBorder
  style={{
    borderRadius: 12,
    boxShadow: [
      {y: 10, blur: 28, color: '#000000b0'},
      {blur: 8, spread: -2, color: '#ffffff30', inset: true},
    ],
  }}
/>
```

| フィールド | 意味 |
| --- | --- |
| `x`, `y` | オフセット（論理ピクセル） |
| `blur` | ぼかし半径（CSS と同じ意味） |
| `spread` | 影を広げる（正）／縮める（負） |
| `color` | 影の色 |
| `inset` | コンポーネントの外側ではなく、パディングボックスの内側に描く |

先頭の要素が最前面に描かれます。外側の影がコンポーネント自身の下に描かれることはなく、
内側の影が枠に重なることもありません。

## 描画順

1 つのコンポーネントの中では、奥から手前へ次の順に描かれます。

1. 外側の `boxShadow`
2. `paint`（または組み込みの背景・枠）
3. 内側の `boxShadow`
4. ラベル・アイコンの中身、続いて子要素
5. `overlay`

[`<ShaderEffect/>`](./components.md#shadereffect) では、エフェクトの出力が組み込みの
背景と枠の間に入ります。独自の `paint` はエフェクト出力の上に描かれ、内側の影は
エフェクトと `paint` の間に入ります。

## エラー

コンパイルに失敗したペイントシェーダーは、エフェクトのシェーダーエラーと同じように
表示されます。ファイルが直るまで、そのコンポーネントは組み込みの見た目で描かれます。

## Rust

Rust の設定 SDK にも同じ API があります。

```rust
use shojiwm_rs::prelude::*;

let hover = signal(0.0);
let glow = paint_shader("shaders/glow.frag").uniform("strength", hover).outsets(16.0);
Flex::row()
    .overlay(glow)
    .style(Style::new().border_radius(8.0).box_shadow(shadow(0.0, 10.0, 28.0, hex("#000000b0"))));
```
