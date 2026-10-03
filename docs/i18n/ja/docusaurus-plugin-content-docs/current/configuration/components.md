---
sidebar_position: 8
---

# SSD コンポーネント

これらは [`COMPOSITOR.window.composition`](./window-composition.md) の内側で組み立てて
ウィンドウ装飾を描くための部品です。`shoji_wm` から import します。

```tsx
import {Box, Label, Button, AppIcon, Image, ShaderEffect, WindowBorder} from 'shoji_wm';
```

`<ManagedWindow/>` と `<ClientWindow/>` は
[ウィンドウの合成](./window-composition.md) ページで解説しています。

## 共通の prop

すべてのコンポーネントが受け付けます（`ComponentProps` 由来）。

| Prop | 型 | 意味 |
| --- | --- | --- |
| `children` | ノード | 子コンポーネント |
| `style` | `SSDStyle` | 視覚スタイル。[スタイルリファレンス](#スタイルリファレンス) を参照 |
| `id` | `string` | ターゲット無効化のための安定したノード id |
| `onHoverChange` | `(hovered: boolean) => void` | ポインターの出入り |
| `onActiveChange` | `(active: boolean) => void` | 押下／解放 |
| `paint` | `PaintShaderHandle` | 組み込みの背景・枠を置き換え。[ペイントシェーダー](./paint.md)を参照 |
| `overlay` | `PaintShaderHandle` | 子要素より上に描画。[ペイントシェーダー](./paint.md)を参照 |

すべての `style` 値（および多くの prop）は、素の値かシグナルのどちらも受け付けるので、
リアクティブに更新されます。

---

## `<Box/>`

子要素を水平または垂直に整列するフレックスボックス風コンテナです。

| Prop | 型 | 意味 |
| --- | --- | --- |
| `direction` | `"row" \| "column" \| "horizontal" \| "vertical"` | レイアウト軸（デフォルト `"row"`） |
| `split` | `Direction` | 2 パネルレイアウトの分割方向 |
| `style` | `SSDStyle` | スタイル |

```tsx
<Box direction="row" style={{gap: 8, padding: 4, alignItems: 'center'}}>
  <AppIcon icon={window.icon} style={{width: 16, height: 16}} />
  <Label text={window.title} style={{flexGrow: 1}} />
</Box>
```

## `<Label/>`

テキスト文字列を描画します。

| Prop | 型 | 意味 |
| --- | --- | --- |
| `text` | `string`（またはシグナル） | 表示するテキスト |
| `style` | `SSDStyle` | `fontSize`・`fontWeight`・`fontFamily`・`color`・`textAlign`・`lineHeight` でフォントと色を指定 |

```tsx
<Label
  text={window.title}
  style={{color: '#f5f7fa', fontSize: 13, fontWeight: 600, fontFamily: ['Noto Sans CJK JP', 'Noto Color Emoji']}}
/>
```

## `<Button/>`

クリックでアクションをトリガーするプレス可能な領域です。

| Prop | 型 | 意味 |
| --- | --- | --- |
| `onClick` | `() => void` または `WindowActionDescriptor` | クリック時のアクション |
| `onHoverChange` | `(hovered: boolean) => void` | 視覚フィードバック用のホバー追跡 |
| `style` | `SSDStyle` | スタイル |

`onClick` はコールバック、または組み込みウィンドウ操作用に `windowAction(...)` が返す
ディスクリプタを受け付けます。操作は `"close"`・`"maximize"`・`"unmaximize"`・
`"minimize"`・`"fullscreen"`・`"unfullscreen"` です。

```tsx
import {Button, windowAction} from 'shoji_wm';

// 組み込みアクション
<Button onClick={windowAction('close')} style={{width: 12, height: 12}} />

// カスタムハンドラ＋ホバーフィードバック
const [hover, setHover] = useState(false);
<Button
  onHoverChange={setHover}
  onClick={() => window.minimize()}
  style={{width: 16, height: 16, borderRadius: 8, background: hover((h) => h ? '#FFFFFF40' : '#FFFFFF20')}}
/>
```

## `<AppIcon/>`

ウィンドウのアプリケーションアイコンを描画します。

| Prop | 型 | 意味 |
| --- | --- | --- |
| `icon` | `WindowIcon \| undefined`（またはシグナル） | `window.icon` を渡すとリアクティブに更新 |
| `style` | `SSDStyle` | サイズ指定 |

```tsx
<AppIcon icon={window.icon} style={{width: 16, height: 16}} />
```

## `<Image/>`

ファイルパス（設定パッケージルートからの相対）またはリアクティブなソースから画像を
表示します。

| Prop | 型 | 意味 |
| --- | --- | --- |
| `src` | `string`（またはシグナル） | 画像パス |
| `fit` | `"contain" \| "cover" \| "fill"` | 画像がボックスを満たす方法 |
| `style` | `SSDStyle` | サイズ／配置 |

```tsx
<Image src="./assets/x.svg" style={{width: 16, height: 16, pointerEvents: 'none'}} />
```

## ShaderEffect

`<ShaderEffect/>` は子要素が占める領域にコンパイル済み GPU エフェクトを適用する
コンテナです。`shader` の作り方は [エフェクト](./effects.md) を参照してください。

| Prop | 型 | 意味 |
| --- | --- | --- |
| `shader` | `CompiledEffectHandle` | 描画するコンパイル済みエフェクト |
| `direction` | `Direction` | 子要素のレイアウト軸（`<Box/>` と同様） |
| `style` | `SSDStyle` | スタイル |

```tsx
<ShaderEffect shader={frostedGlass} direction="row" style={{height: 28, paddingX: 8, alignItems: 'center'}}>
  <Label text={window.title} />
</ShaderEffect>
```

## WindowBorder

`<WindowBorder/>` は `<ClientWindow/>` の周囲に置き、ボーダーを描画してインタラクティブな
リサイズの当たり判定を提供するクロムコンテナです。

| Prop | 型 | 意味 |
| --- | --- | --- |
| `style` | `SSDStyle` | `border`・`borderRadius`・`background` などでボーダーを指定 |
| `interaction` | `WindowBorderInteraction` | リサイズの当たり判定 |

`interaction.resizeHitArea` は単一の数値か、`{edgePx?, cornerPx?}`（エッジ沿いと
コーナーのつかみ厚）です。

```tsx
<WindowBorder
  style={{border: {px: 2, color: borderColor}, borderRadius: 10}}
  interaction={{resizeHitArea: {edgePx: 8, cornerPx: 14}}}
>
  <ClientWindow />
</WindowBorder>
```

---

## スタイルリファレンス

`style` prop は `SSDStyle` です。すべての値はシグナルにできます。長さは特記なき限り
論理ピクセルです。

### サイズ

`width`・`height`（数値または `"100%"` のような文字列）・`minWidth`・`minHeight`・
`maxWidth`・`maxHeight`・`flexGrow`・`flexShrink`。

### 余白

`gap`・`padding`・`paddingX`・`paddingY`・`paddingTop/Right/Bottom/Left`・`margin`・
`marginX`・`marginY`・`marginTop/Right/Bottom/Left`。

### レイアウトと位置

`alignItems`（`"start" | "center" | "end" | "stretch"`）・`justifyContent`
（`"start" | "center" | "end" | "space-between"`）・`position`
（`"relative" | "absolute"`）・`inset`・`top`・`right`・`bottom`・`left`・`zIndex`・
`overflow`（`"visible" | "hidden"`）・`pointerEvents`（`"auto" | "none"`）・
`transform`（`{translateX, translateY, scale, scaleX, scaleY}`）。

### 外観

`background`・`color`・`opacity`・`visible`・`cursor`・`borderRadius`。

ボーダー: `border`・`borderTop`・`borderRight`・`borderBottom`・`borderLeft`（各
`{px, color}`）に加えて `borderFit`（`"normal" | "fit-children"`）。
辺ごとの指定はその辺について `border` を上書きし、幅（コンテンツ領域もそれに合わせて
ずれます）と色の両方に効きます。0 でない幅は最低でも物理 1px になります。

影: `boxShadow` — `{x, y, blur, spread, color, inset}` 1 つ、またはその配列。CSS と
同じ意味です。[ペイントシェーダー](./paint.md#影-boxshadow)を参照。

色は `#RGB`・`#RGBA`・`#RRGGBB`・`#RRGGBBAA` で指定します。

### テキスト（`<Label/>` 用）

`fontSize`・`fontWeight`（`"normal" | "medium" | "semibold" | "bold"` または数値）・
`fontFamily`（文字列、またはフォールバックの文字列配列）・`textAlign`
（`"start" | "center" | "end"`）・`lineHeight`。

```tsx
const style: SSDStyle = {
  height: 28,
  paddingX: 8,
  gap: 8,
  alignItems: 'center',
  background: window.isFocused((f) => (f ? '#1f2430cc' : '#2a2f3acc')),
  borderRadius: 8,
};
```
