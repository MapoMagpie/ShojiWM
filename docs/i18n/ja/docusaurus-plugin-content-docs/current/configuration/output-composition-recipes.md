---
sidebar_position: 10.62
---

# 出力の合成レシピ集

[出力の合成](./output-composition.md)で作る演出の完成形です。どれも演出していない間は
`null` を返す関数になっているので、そのときは既定の重なりに戻します。こうすると
フルスクリーンのファストパスも保たれます:

```tsx
COMPOSITOR.rendering.composition = (output) =>
  crossfade(output) ?? <DefaultComposition />;
```

合成関数そのものからは `null` ではなく既定を返してください。ノードが 1 つも無い合成は
何も描きません。

以下の名前はすべて `shoji_wm` から import できます。

## 任意の 2 つのビュー間のキューブ

`examples/output-composition/cube-transition.tsx` は、任意の 2 つの合成（2 つの
ワークスペースなど）の間でキューブを回します:

```tsx
import { COMPOSITOR, DefaultComposition, Layers, Windows } from "shoji_wm";
import { createCubeTransition } from "./cube-transition";

const cube = createCubeTransition({ duration: 700 });
COMPOSITOR.rendering.composition = (output) =>
  cube.compose(output) ?? <DefaultComposition />;

// 切り替えるとき（ウィンドウ自体はアニメーションなしで切り替える）:
cube.start(outputName, {
  from: <><Layers layers={["background", "bottom"]} /><Windows windows={fromIds} /></>,
  to: <><Layers layers={["background", "bottom"]} /><Windows windows={toIds} /></>,
  direction: 1,
});
```

## バー以外を暗くする

テクスチャは要りません。ウィンドウとバーの間に半透明の塗りを挟みます。

```tsx
const [dimmed, setDimmed] = signal(false);

COMPOSITOR.rendering.composition = () => (
  <>
    <Layers layers={["background", "bottom"]} />
    <Windows />
    {dimmed() && <Solid color="#00000099" />}
    <Layers layers={["top", "overlay"]} />
    <LayerPopups />
  </>
);
```

ルートに `<Windows />` が残っているので、塗りが無い間はフルスクリーンのファストパスも
働きます。

## クロスフェード

2 つのテクスチャビューで、あるビューのスナップショットから別のビューへフェードします:

```tsx
const [fade, setFade] = signal<number | null>(null); // 0 → 1

function crossfade(output: OutputInfo) {
  const t = fade();
  if (t === null) return null;
  const from = renderTexture({ key: "fade-from", content: <Windows windows={fromIds} /> });
  const to = renderTexture({ key: "fade-to", content: <Windows windows={toIds} /> });
  return (
    <>
      <Layers layers={["background", "bottom"]} />
      <TextureView texture={from} opacity={1 - t} />
      <TextureView texture={to} opacity={t} />
      <Layers layers={["top", "overlay"]} />
      <LayerPopups />
    </>
  );
}
```

`fade` は出力の `createPoll` で動かし、終わったら `null` に戻します。

## ズーム

デスクトップのテクスチャを画面上の 1 点を中心に拡大します。3D シーンは平面を出力の
範囲に切り取るので、倍率はいくらでも構いません:

```tsx
function zoomed(output: OutputInfo) {
  const k = zoom(); // 1 = 等倍
  if (k <= 1) return null;
  const { width, height } = outputLogicalSize(output);
  const desktop = renderTexture({ key: "zoom", content: <DefaultComposition />, scale: 2 });
  // 注目点のワールド座標（原点は中央、+Y が上）。
  const cx = focusX() - width / 2;
  const cy = height / 2 - focusY();
  return (
    <Scene3D camera={screenCamera(output)}>
      <Plane texture={desktop} width={width} height={height}
        transform={transform3d().translate(cx * (1 - k), cy * (1 - k)).scale(k)} />
    </Scene3D>
  );
}
```

`scale: 2` でテクスチャを 2 倍の画素密度で描くので、2 倍までは文字も鮮明です。入力は
拡大されず、ウィンドウの実際の位置に届きます。

## ウィンドウごとのテクスチャ

テクスチャには 1 つのウィンドウだけを収めることもできます。テクスチャをウィンドウの
大きさにし、`offsetX`/`offsetY` でウィンドウをテクスチャの角に合わせます。影のぶんの
余白を取っておきます。

```tsx
const MARGIN = 48;

function windowTexture(output: OutputInfo, window: WaylandWindow) {
  const { x, y, width, height } = window.position;
  return renderTexture({
    key: `window-${window.id}`,
    width: width + MARGIN * 2,
    height: height + MARGIN * 2,
    content: (
      <Windows
        windows={[window]}
        offsetX={output.position.x - x + MARGIN}
        offsetY={output.position.y - y + MARGIN}
      />
    ),
  });
}
```

こうしたテクスチャはそのウィンドウが変わったときだけ描き直されるので、十数枚あっても
止まっている間はほとんどコストがかかりません。ID 指定なので、最小化中や別ワーク
スペースのウィンドウも映ります。

### 3D ウィンドウスイッチャー

ウィンドウごとのテクスチャがあれば、Windows Vista の「フリップ 3D」風のスイッチャーは
平面を並べるだけです:

```tsx
const [selected, setSelected] = signal<number | null>(null);

function flipComposition(output: OutputInfo, windows: WaylandWindow[]) {
  const index = selected();
  if (index === null || windows.length === 0) return null;
  const { height } = outputLogicalSize(output);
  // 選択中のウィンドウを手前に、次のものほど奥に。
  const ordered = windows.map((_, i) => windows[(index + i) % windows.length]);
  return (
    <>
      <Layers layers={["background", "bottom"]} />
      <Scene3D camera={screenCamera(output, { distance: height * 0.6 })}>
        {ordered.map((window, depth) => (
          <Plane
            texture={windowTexture(output, window)}
            width={window.position.width + MARGIN * 2}
            height={window.position.height + MARGIN * 2}
            transform={transform3d()
              .translate(depth * 90 - 150, depth * 40, -depth * 260)
              .rotateY(-30)}
          />
        ))}
      </Scene3D>
      <Layers layers={["top", "overlay"]} />
      <LayerPopups />
    </>
  );
}

COMPOSITOR.key.bind("flip-next", "Super+Tab", () =>
  setSelected(((selected.peek() ?? -1) + 1) % Math.max(1, myWindows().length)));
COMPOSITOR.key.bind("flip-pick", "Super+Return", () => {
  const index = selected.peek();
  if (index !== null) myWindows()[index]?.focus();
  setSelected(null);
});
```

ほかのレシピと同じように `flipComposition(output, myWindows()) ?? <DefaultComposition />`
で使います。`myWindows()` は手元のウィンドウ一覧です（ウィンドウマネージャーが持つ現在の
ワークスペースのウィンドウなど）。並びのアニメーションは `createPoll` で平行移動を
補間します。

スイッチャーの表示中も、マウスはウィンドウの実際の位置に届きます。操作はキーボードで
行ってください。

**ウィンドウごとのテクスチャの中のブラー。** テクスチャにウィンドウが 1 つだけだと、
その下には何も無いので、そのウィンドウの backdrop ブラー（ガラスのタイトルバーなど）は
空をぼかすだけで、それでもウィンドウごとにブラー 1 回ぶんのコストがかかります。
スイッチャーを開いている間は、エフェクトを選ぶ箇所で同じ signal を読んで、backdrop の
無いエフェクトに切り替えてください:

```ts
COMPOSITOR.effect.window = (window) =>
  selected() !== null ? { behind: TINT } : { behind: GLASS };
```
