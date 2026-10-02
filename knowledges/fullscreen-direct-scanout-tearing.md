# Fullscreen + Direct Scanout + Tearing

フルスクリーンのファストパス（Direct Scanout）と、その上での Tearing（即時 / async page flip）を
実装する過程でハマった問題と、その根本原因・修正のまとめ。目的は「フルスクリーンのゲームを
できるだけ低レイテンシ（"Windows より軽い"）に出す」こと。

関連ファイル:

- Rust: `src/shojiwm_lib/src/backend/tty.rs`（描画スケジューラ／scanout／tearing 判定）
- Rust: `src/shojiwm_lib/src/protocols/tearing_control.rs`（`wp_tearing_control_v1`）
- Rust: `src/shojiwm_lib/src/ssd/window_model.rs`（`ManagedWindowState`、`allow_tearing`）
- TS: `packages/shoji_wm/src/types.ts` / `reconcile.ts`、`packages/config/src/index.tsx`
- Smithay 側パッチ: 別リポジトリ `smithay-tearing`（`backend/drm` の async flip サポート）

全体の流れは「Fullscreen fast path（シーンを overlay + bare client surface に折り畳む）」→
「Direct Scanout（client buffer をプライマリプレーンへ zero-copy で載せる）」→
「Tearing（その frame を async page flip で出す）」の三段。各段でそれぞれ別の罠があった。

---

## 1. Direct Scanout が一向に発動しない（3 つの根本原因）

### Symptoms

- フルスクリーンにしても `direct scanout engaged` ログが出ない、もしくは ~30Hz で
  engage/disengage を繰り返してチラつく。
- client buffer がプライマリプレーンに載らず、常に GL で合成されている。

### Root causes（3 つ全部直して初めて安定）

1. **dmabuf の node が `None` で `NodeFilter::Node(_)` に一致しない。**
   Smithay の `wayland/dmabuf/dispatch.rs` は `create_dmabuf(..., None)` で dmabuf の node を
   ハードコードで `None` にしている。`can_add_framebuffer`（gbm）が
   `import_node == dmabuf.node()` を比較するため、`NodeFilter::Node(_)` は永遠に一致しない。
   → scanout フィードバックの tranche では `NodeFilter::All` を使う必要がある。

2. **フルスクリーン中の clear color が不透明領域として広告されない。**
   通常の clear color（非黒）のままだと、フルスクリーン client が出力全体を覆っていても
   opaque region として扱われず、`try_assign_primary_plane` の
   `crtc_background_matches_clear_color`（黒/透明）も成立しない。
   → フルスクリーン時は `frame_clear_color` を黒 `[0,0,0,1]` に差し替える（`tty.rs`）。
   フルスクリーン client が全面を覆うので clear color は実際には見えない。

3. **フルスクリーン中の `DamageOnlyElement` が scanout/swapchain のフィードバックループを作る。**
   これが ~30Hz の engage/disengage フラッピングの原因。フルスクリーン中は
   damage-only element を抑制する。

### 確認

```
direct scanout engaged: client buffer assigned to primary plane (zero-copy) ... fullscreen_fast_path=true
fullscreen fast path engaged: scene collapsed to overlay layers + bare client surface
```

エッジトリガのログ（`note_direct_scanout_transition` / `note_fullscreen_fast_path_transition`）は
通常運用ログとして残してある。

---

## 2. NVIDIA 圧縮 modifier で Direct Scanout ↔ fallback がフレーム毎に交互に出る

### Symptoms

- NVIDIA 環境（RTX 50 系）で、フルスクリーン中に Direct Scanout が数フレーム毎に
  engage / disengage を繰り返し、操作性が著しく悪化する（ファストパスと fallback で
  レイテンシ/cadence が違うため、視点移動やポインタが不安定に感じる）。

### Root cause

NVIDIA の block-linear modifier は bits 25:23 に **可逆フレームバッファ圧縮**をエンコードする。
カーネルはこの圧縮 modifier を「プレーン対応」として広告するので、client（ゲーム）は喜んで
圧縮バッファを確保する。ところが、それを実際にプライマリプレーンへ載せる atomic commit は
**ドライバに拒否される**ため、GL fallback に落ちる。テアリング/高 FPS の client では
allocator が毎フレーム同じ modifier を選ぶとは限らないので、「直接 scanout 可能 ↔ GL fallback 必要」を
延々と往復する。

### Fix

dmabuf **scanout** フィードバックの tranche から NVIDIA 圧縮 modifier を除外する
（`is_nvidia_compressed_modifier` in `tty.rs`、`surface_dmabuf_feedback` でフィルタ）。
こうすると client は「実際に flip できる modifier」に留まり、Direct Scanout が安定して engage し続ける。

```rust
// vendor は最上位バイト、圧縮は bits 25:23
modifier >> 56 == 0x03            // NVIDIA
  && modifier & 0x10 != 0         // BLOCK_LINEAR_2D
  && modifier & (0x7 << 23) != 0  // COMPRESSION_MASK
```

> Intel の render-compressed modifier も別途、screencopy/PipeWire 消費者向けに
> LINEAR/INVALID へ絞る処理がある（`tty.rs` 内、別の関心事）。

---

## 3. Tearing 有効時に EINVAL でクラッシュ（async flip がカーソルプレーンに触れる）

### Symptoms

- フルスクリーンでしばらくすると
  `DrmError(Access(... EINVAL ...))` … `no prop can be changed during async flip`（crtc_x 等）で落ちる。

### Root cause

DRM の async page flip（`PAGE_FLIP_ASYNC` / `PageFlipFlags::ASYNC`）は
**プライマリプレーンしか触れない**。同じ commit でカーソルプレーン（crtc_x など）を変更すると
カーネルが EINVAL で拒否する。Hyprland も同じ制約を持つ（ドキュメント化済み）。

### Fix（最終形：software cursor + 同期フリップへのフォールバック）

- Tearing 中は frame flags から `ALLOW_CURSOR_PLANE_SCANOUT` を外し、**カーソルをフレームに
  合成（software cursor）**する（Hyprland 方式）。ゲームがポインタを grab/hide していれば
  そもそもカーソルは出ないので、bare client buffer がそのまま direct-scanout される。
- 念のため Smithay 側 `submit()` で、async flip がドライバに拒否されたら **その frame を
  同期フリップで再試行**する（atomic は all-or-nothing なので失われた frame は無い。
  `async (tearing) page flip rejected by the driver; retrying as a synced flip`）。

過渡的に試した「カーソルが動いたら tearing を諦めて同期フリップ」案は妥協が大きく
（カーソル frame と game frame の cadence が混ざって却って不均一）、不採用。
Hyprland 同様の software cursor に統一した。

### 関連: should_tear のガード

```
should_tear = supports_async_flip && !cursor_visible && fullscreen_scanout && (...)
```

`!cursor_visible` がガードなので、マウスを動かしてカーソルが一瞬出る → tearing 一時停止 →
隠れる → 再開、という短い engage/disengage は**仕様どおりの一過性**であり問題ではない。

---

## 4. クラッシュがログに出ない（panic ではなく Err 伝播だった）

### Symptoms

- 上記クラッシュ時、`logs/latest.log` にスタックトレースが出ず、原因が掴めない。

### Root cause

panic ではなく、描画/フリップ失敗が `Result` として `main() -> Result` まで伝播し、
Rust のデフォルト `Termination` が stderr に出して終了していた。`main.rs` の panic hook は
**panic しか拾わない**ので、この `Err` 伝播は素通りしていた。

### Fix

TTY メインループの描画呼び出しを `if let Err(err) = render_if_needed(...) { error!(...); return Err(err) }` で
包み、tracing ログに残してから落ちるようにした（`backend/mod.rs`）。以降クラッシュ原因が
ログに残る。

---

## 5. 【本命】Tearing 中の present が不均一になる（estimated-vblank throttle が commit を詰まらせる）

これが一番厄介で、視点をグリグリ動かすと「光源の残像が等間隔に出ない」＝60Hz相当に見える、
という症状。Hyprland では等間隔に出るので ShojiWM 側の問題と判明。

### Symptoms

- present rate 自体は最大 290Hz 以上出ており「80Hz cap」ではない（**当初これを最初の不安定な
  ログだけ見て誤診した**）。問題は present **間隔**の不均一。
- 高レート窓で「0.5ms に 2 フレーム束化 → ~7.7ms（≈144Hz vblank 周期）ギャップ」という
  規則的なパターン。クライアントは ~1ms 毎に新バッファを commit しているのに present されない。
- **マウス視点移動で特に顕著、キー移動だと滑らか**（重要な手がかり）。

### Root cause

1. `frame_finish` は async flip 完了後すぐに再描画する（commit 駆動の即時再描画）。
   flip 完了は ~0.3ms、client の次 commit は ~1ms 後なので、この即時再描画はしばしば
   **次のバッファが来る前**に走り、**no-damage フレーム**になる。
2. no-damage パスは surface を `WaitingForEstimatedVBlank` に置き、**≈1リフレッシュ周期(6.94ms)の
   タイマー**を仕込む（`schedule_estimated_vblank_callback`）。
3. その間に来る client commit は `queue_tty_redraws` で `queued = true` にされるだけで
   `Queued` に昇格せず、`render_surface` がスキップされる。→ タイマーが切れるまで present されない。
   実効 present cadence が ~refresh rate に潰れ、フレームが不均一に束化する。
4. **マウス特有の理由**: Minecraft は xwayland-satellite 経由（X11）で Wayland ポインタを
   ロックしないため、マウス移動の度に通常経路の `schedule_redraw()`（`input.rs`）が走り、
   `redraw_needed` を立て、上記 no-damage→throttle を多発させる。キー移動はポインタイベントが
   無いので throttle されにくい。残像の不均一はマウス回転時に知覚的に特に目立つ。

### Fix（`queue_tty_redraws`）

Tearing 中（`surface.tearing_active`）の surface が `WaitingForEstimatedVBlank` にいるとき、
commit/redraw が来たら **即 `Queued` へ昇格**させ、タイマーを待たず present する。
古い estimated-vblank タイマーは、実フレーム queue 時の `frame_callback_timer_generation`
バンプによる generation ガードで no-op 化されるので安全。タイマー自体は残すので、
クライアントが本当に commit を止めた（ポーズ等）場合の frame-callback セーフティネットも維持。

```rust
TtyRedrawState::WaitingForEstimatedVBlank { generation, .. } => {
    surface.redraw_state = if tearing_active {
        TtyRedrawState::Queued            // commit を refresh 周期まで待たせない
    } else {
        TtyRedrawState::WaitingForEstimatedVBlank { queued: true, generation }
    };
}
```

これは tearing present ループの片割れ。もう片方は `frame_finish` の即時再描画
（前の flip 完了直後に最新バッファを submit）。両者で present cadence を refresh rate ではなく
client commit rate に追従させる。

### 結果（修正前 → 後、計測）

| 指標 | 修正前 | 修正後 |
|---|---|---|
| present rate p50 | ~113Hz | **280Hz** |
| present rate p90 | 144Hz | **372Hz** |
| 高レート窓の平均間隔 | 4〜5ms | **2.5ms** |
| 〃 max interval | 9〜17ms | **6〜8ms** |

min/max 比が ~30倍 → ~8倍に収束し、大きなギャップが消えた。視点移動の残像が等間隔になった。

### 未実施の二段目改善案

`frame_finish` の即時再描画を「実際に新バッファがある時だけ」に絞る commit カウンタ方式
（`with_renderer_surface_state(surface, |s| s.current_commit())` を保存した
`last_present_window_commit` と比較）。no-damage 再描画自体を無くし、マウス移動時の無駄レンダ
（~1000Hz）を削減できる。今回の修正で refresh-rate cap は外れているので、これは効率/均等性の
追い込み。`queue_tty_redraws` の該当コメントにもコード内に残してある。

---

## 6. TS API（per-app の `allowTearing`）

per-window の tearing 許可は `<ManagedWindow allowTearing>` prop で与える
（`interactive` / `tiled` と同じ `MaybeSignal<boolean>` パターン）。
`reconcile.ts` → `ManagedWindowState.allowTearing` → Rust `ManagedWindowState.allow_tearing: Option<bool>`。

セマンティクスは **Model B（config が真実の源）**:

```
should_tear = supports_async_flip && !cursor_visible
  && fullscreen_scanout.is_some_and(|w| {
        SHOJI_FORCE_TEARING                       // env 強制（テスト用）
     || config.allow_tearing                      // Some が優先（config が真実の源）
        .unwrap_or(client が wp_tearing_control)   // 未設定時のみ client ヒントにフォールバック
  })
```

- `allowTearing` が config で設定されていればそれが client の `wp_tearing_control` ヒントを上書きする。
  未設定（`undefined`/`None`）の時だけ client ヒントにフォールバック。
- これにより **`wp_tearing_control` を送らない X11/Xwayland のゲーム（Minecraft 等）でも
  config だけでテアリングできる**。これが A 案（client ヒントを AND するだけ）ではなく B 案を
  選んだ理由。
- デフォルトは `config/index.tsx` のフルスクリーン分岐で `allowTearing={true}`。実際にテアリングが
  起きるのは「フルスクリーン＋direct scanout＋カーソル非表示＋refresh 超えの commit」が
  揃った時だけなので、通常のフルスクリーンアプリでは実質 no-op。

`forced=false` で `tearing engaged` ログが出ていれば、env ではなく config 経由で駆動されている証拠。

---

## 7. Smithay 側パッチ（async flip サポート）の構成

Tearing には Smithay の `backend/drm` 改修が必要で、ShojiWM 本体とは別管理。

- `DrmSurface::page_flip(..., tearing)` が `PAGE_FLIP_ASYNC`（atomic）/
  `PageFlipFlags::ASYNC`（legacy）をセット。
- `DrmSurface::supports_async_page_flip()` がドライバ capability
  （`AtomicASyncPageFlip` / `ASyncPageFlip`）を問い合わせ。
- `DrmCompositor::queue_frame_tearing(user_data, tearing)`（`queue_frame` は `tearing=false` で委譲）と
  `DrmCompositor::supports_async_page_flip()`。
- `submit()` で async 拒否時に同期フリップへフォールバック（§3 参照）。

パッチは **クリーンな upstream master 上の単一コミット**として `smithay-tearing` リポジトリの
`tearing` ブランチに置く（ShojiWM 専用パッチは混ぜない）。その上に ShojiWM 専用パッチを重ねた
別ブランチを GitHub から参照する運用。

> 移植時の注意: fork 側と clean upstream で `submit()` 内 `handle_flip` のシグネチャが異なるため、
> `compositor/mod.rs` だけ `git apply` が通らず手動適用が必要だった。tearing 固有の変更
> （`tearing` フィールド / `queue_frame_tearing` / async retry / `supports_async_page_flip`）のみを
> 当て、ベース差分の `handle_flip` 周りには触れないこと。

---

## デバッグ用 env / 確認ポイント

| env | 用途 |
|---|---|
| `SHOJI_FORCE_TEARING=1` | config の `allowTearing` に関係なくフルスクリーンで tearing 強制（テスト用） |
| `SHOJI_PRESENT_RATE_DEBUG` | present レートの per-second 統計 |

> 詳細な present cadence 計測ログ（`tearing timing` / `queue cadence` / `tearing pipeline` 等）は
> 調査用に大量に入れていたが、原因特定後に掃除した。残した通常ログは edge-triggered な
> `direct scanout engaged/disengaged` / `fullscreen fast path engaged/disengaged` /
> `tearing engaged/disengaged`（`forced=` 付き）。

確認の定石:

1. `fullscreen fast path engaged` → `direct scanout engaged ... fullscreen_fast_path=true`（zero-copy）→
   `tearing engaged ... forced=false` が順に出るか。
2. `tty surface async page-flip (tearing) capability ... supports_async_flip=true` が出力毎に出ているか。
3. クラッシュ時は `latest.log` に `EINVAL` / `retrying as a synced flip` / `tty render iteration failed` が
   出ていないか。
