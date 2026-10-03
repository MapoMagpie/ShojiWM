---
sidebar_position: 10.5
---

# 出力オーバーレイ

`COMPOSITOR.effect.overlay(output, options)` は、既存の ShojiWM シェーダーパイプラインを
使って出力（画面）全体にエフェクトを描きます（TTY・Winit 両対応）。Wayland サーフェス・
入力領域・キーボードフォーカスは作りません。既存のウィンドウ・レイヤー・ポップアップ・
背景エフェクトの API と `shader_main(EffectContext)` の約束事はそのままです。

```ts
const handle = await COMPOSITOR.effect.overlay("DP-1", {
  effect: compileOverlayEffect({
    input: snapshotSource(),
    alpha: "preserve",
    pipeline: [shaderStage(loadShader("./src/effect/output-dissolve.frag"), {
      uniforms: { progress }, // この遷移が持つシグナル
      // textures: { live: backdropSource() },
    })],
  }),
  placement: "top",
  maxDuration: 10_000,
});
// この時点で直前のフレームが撮影され、シェーダーも一度成功している
changeScene();
// progress を動かしてからエフェクトを解放する（実際は try/finally で）
handle.dispose();
```

ディゾルブの完全な例は `examples/output-overlay/` にあります。2 つのファイルを設定の
`src/effect/` にコピーし、同期的なキー／イベントのコールバックから
`void transition(outputName, changeScene).catch(console.error)` を呼んでください。
例では `progress` をシグナルと単純なタイマーループで動かしています。

**オーバーレイをモジュールのトップレベルで await したり、コンポジタが完了を待つ
コールバック（`onPointerMoveAsync`、`onGestureSwipeAsync` など）からその Promise を
返したりしないでください。** 次のフレームを作るには、メインスレッドが描画に戻る
必要があります。オーバーレイの作成はタイマー 1 回分だけ待って、切り離されたコールバックが
終わるのを待ちます。そのあとも呼び出し元のコンポジタリクエストを処理中なら reject します。
モジュール初期化中の呼び出しも reject されます。安全策として、ネイティブ側の期限も
描画とは独立に働きます。上の例のように、遷移は切り離したタスクとして起動してください。

## ソースと配置

| オプション | 動作 |
| --- | --- |
| `snapshotSource()` | その配置位置でのシーンを凍結した撮影結果。同じ枠の前のオーバーレイも含み、カーソルは含まない。リクエストごとに 1 回だけ撮影。 |
| `backdropSource()` | この枠より奥のライブなシーン。参照されていて、パイプラインの再評価が必要なときだけ撮影。自身の前回の出力は含まない（フィードバック防止）。 |
| `shaderInput()`、`imageSource()`、状態テクスチャ | 既存の生成系・マルチパス入力。画面ソースを参照しない限り画面は撮影しない。 |
| `top`（既定） | デスクトップのウィンドウと layer-shell サーフェスの上、カーソルの下。コンポジタの診断表示は手前に残ることがある。 |
| `below-layers` | ウィンドウ（とそのデコレーションのポップアップ）の上、Top/Overlay の layer-shell サーフェスとレイヤーポップアップの下。Bottom/Background レイヤーはウィンドウの奥のまま。 |

`below-layers` は「ウィンドウの下」という意味では**ありません**。ウィンドウより下の枠はありません。

フェードや透過する手続き的な出力には `alpha: "preserve"` を使います。既存のレイヤー
エフェクトと同じく、ピクセルは乗算済みアルファです。`capturePadding` は 0 でなければ
なりません。window/layer/popup/xray 入力は出力エフェクトでは対象がないため拒否されます。
`snapshotSource()` は主入力だけでなく、シェーダーの名前付きテクスチャにも使えます。
スナップショットを使う／使わないを切り替えるには、新しいオーバーレイが必要です。
既存のコンパイル済みエフェクトと同じく、主入力が生成シェーダーでない限り
パイプラインのステージは 1 つ以上必要です（空のスナップショットパイプラインは
ネイティブのデコーダーが拒否します）。

## 常駐するライブエフェクト

`persistent: true` にすると、最初のフレームが成功したあともオーバーレイが残ります。

```ts
const pixelSize = signal(12); // 物理テクスチャピクセル
const handle = await COMPOSITOR.effect.overlay("DP-1", {
  persistent: true,
  placement: "top", // ウィンドウ、デスクトップウィジェット、layer-shell UI を含む
  effect: compileOverlayEffect({
    input: backdropSource(),
    alpha: "preserve",
    pipeline: [shaderStage(loadShader("./src/effect/output-pixelation.frag"), {
      uniforms: { pixelSize },
    })],
  }),
});
pixelSize.value = 24;
// 最初のフレーム以降は寿命の期限がないので、明示的に無効化する
handle.dispose();
await handle.closed;
```

作成時の「切り離したタスクから」という制約は一時的なオーバーレイと同じです。この
モードの `maxDuration`（既定 10 秒）は最初のフレームが成功するまでの時間だけを制限し、
有限の正の値である必要があります。準備ができた時点で期限は外れます。定期的に
作り直したり、大きなタイムアウトを与えたりはしません。期限付きの元の寿命にするには、
`persistent` を省略するか `false` にします。

出力と配置の組ごとに枠が 1 つあります。複数のモニターにかけるには、出力ごとに
ハンドルを作ってください。`top` は layer-shell のウィジェットや UI を含み、カーソルは
エフェクトの上に残ります。`below-layers` は Top/Overlay の UI に触れません。どちらの
配置も入力サーフェスを作らず、ポインター／キーボードのイベントも横取りしません。
ライブの撮影は、この枠の結果を差し込む前の、合成したてのシーンから取ります（置き換え時も
同じ）。そのため自分の前回のフレームを読むことはありません。下の枠のオーバーレイの結果を、
意図的に上の枠へ渡すことはできます。

`examples/output-overlay/` の `output-pixelation.ts` / `output-pixelation.frag` は、
出力ごとの有効化・無効化・切り替え・ピクセルサイズ変更を export しています。
2 つのファイルを `src/effect/` にコピーし、設定から import して同期的なコールバックで
使ってください。

```ts
COMPOSITOR.key.bind("pixelation", "Super+P", () => togglePixelation("DP-1"));
COMPOSITOR.key.bind("pixelation-coarse", "Super+Alt+P", () => setPixelSize("DP-1", 24));
// イベントのコールバック内で、2 つの出力を個別に有効化する
void enablePixelation("DP-1", 12).catch(console.error);
void enablePixelation("HDMI-A-1", 8).catch(console.error);
// あとで: disablePixelation("DP-1");
```

この例は既定のソースダメージによる無効化を使います。シーンが静止していればキャッシュ
した結果を再利用し、ライブの内容や uniform が変わると再評価します。例にアニメーション
用のタイマーはありません。レンズや屈折のシェーダーも同じモードと `backdropSource()` で
書けます。変えるのはシェーダーと uniform だけで、コンポジタのコードは不要です。
シーンのダメージと無関係にアニメーションさせたい場合は `invalidate: {kind: "always"}`
を指定し、他のエフェクトと同じく時間をシグナルで渡してください。

## 更新・中断・後片付け

uniform とエフェクト記述子の中のシグナルは、ワークスペース設定を再評価せずに
動作中のオーバーレイを更新します。複数の変更は最新の記述子 1 つにまとめられ、GPU
プログラムと状態テクスチャは既存のパイプラインキャッシュを使います。毎フレーム
描き直す必要があるパイプライン（時間方向の状態テクスチャなど）には
`invalidate: {kind: "always"}` を使ってください。時間の uniform は自動では入らないので、
シグナルで明示的に渡します。既定では、入力やシグナルが変わるまで結果を再利用します
（ライブソースの不透明度や変形の変化も含む）。

出力と配置の組ごとに、動作中のオーバーレイは 1 つです。置き換えは、新しいシェーダーが
成功してから古いものを解放します。新しいエフェクトが `snapshotSource()` を使う場合は、
先に古いものの見た目を撮影します。ライブの背景は同じ枠の古いものを含みません。撮影や
シェーダーが失敗すると新しい Promise が reject され、前のオーバーレイはそのまま残り、
失敗した結果は差し込まれません。作成後のエラーは、そのインスタンスを閉じます。

`dispose()` は何度呼んでも安全です。`closed` は、dispose・置き換え・期限切れ・出力の
取り外し・モード／スケール／変形の変更・セッションの一時停止／ロック・設定のリロードで
解決されます。`maxDuration` は既定 10 秒で、有限の正の値でなければならず、最初の
フレームを待つ時間も含みます。常駐オーバーレイでは準備完了で期限が終わりますが、
それ以外の後片付け条件はすべて有効で、ロック解除しても自動では作り直されません。
どの終わり方でもスナップショットとパイプラインの状態は解放され、オーバーレイの
リクエストがないときは撮影もタイマーも動きません。

ジェスチャーがキャンセルされたら、progress を 0 に戻してから dispose してください。
ワークスペースなどシーンの状態を元に戻すのは呼び出し側の責任です。

## 制限

- `snapshotSource()` を受け付けるのは `COMPOSITOR.effect.overlay` だけです。ウィンドウ・
  レイヤー・ポップアップ・`<ShaderEffect/>` のエフェクトで使うと拒否されます。
- オーバーレイには TypeScript ランタイムが必要です。Rust の設定 SDK にはまだ
  オーバーレイの API がありません。
- セッションのロック中は描画されず、カーソルを覆うこともありません。
