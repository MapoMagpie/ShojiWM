import { isSignal, type ReadonlySignal } from "./signals";

export interface PollHandle {
  cancel(): void;
  readonly cancelled: boolean;
  /**
   * Time of the current run on the poll's output clock: the presentation time
   * of the frame this run belongs to.
   * 現在の実行の時刻（その poll の出力の時計）。この実行が属するフレームの表示時刻です。
   */
  readonly nowMs: number;
}

export type PollCallback = (handle: PollHandle) => void;
export type PollDirtyMode = "runtime" | "none";

/**
 * The output a poll is bound to: an output name, a signal of one (the poll
 * follows it, e.g. `window.output`), or a function read on every tick.
 * `undefined` means "no output right now"; the poll then runs on wall-clock
 * timers until it has one again.
 *
 * poll を結び付ける出力。出力名、出力名の signal（`window.output` など。変化に追従）、
 * または毎 tick 読まれる関数。`undefined` は「現在出力なし」で、その間は実時刻の
 * タイマーで実行されます。
 */
export type PollOutput =
  | string
  | ReadonlySignal<string | undefined>
  | ReadonlySignal<string>
  | (() => string | undefined);

export interface PollOptions {
  /**
   * The output whose frames drive this poll. A poll runs at most once per
   * frame of that output, stamped with the frame's presentation time, so its
   * steps are evenly spaced on screen and identical from run to run.
   *
   * この poll を駆動する出力。poll はその出力のフレームごとに最大 1 回、
   * フレームの表示時刻で実行されるため、画面上の刻みが等間隔で毎回同じになります。
   */
  output: PollOutput;
  /**
   * `"runtime"` (default) re-evaluates the config after each run; `"none"`
   * leaves it to whatever the callback marks dirty itself.
   * `"runtime"`（既定）は実行ごとに設定を再評価します。`"none"` はコールバック自身が
   * dirty にしたものだけを更新します。
   */
  dirty?: PollDirtyMode;
}

export interface OutputPollOptions {
  dirty?: PollDirtyMode;
}

export type OutputPollCallback = (handle: PollHandle, outputName: string) => void;

interface SchedulerBridge {
  registerPoll(
    intervalMs: number,
    callback: PollCallback,
    dirtyMode: PollDirtyMode,
    output: () => string | undefined,
  ): PollHandle;
  registerOutputPollGroup(
    intervalMs: number,
    callback: OutputPollCallback,
    dirtyMode: PollDirtyMode,
  ): PollHandle;
}

let activeBridge: SchedulerBridge | null = null;

export function installSchedulerBridge(bridge: SchedulerBridge | null): void {
  activeBridge = bridge;
}

/**
 * Run `callback` every `intervalMs` on the frames of `options.output`.
 *
 * `options.output` is required: time is kept per output (each output's clock
 * is the presentation time of its own frames), and a poll with no output
 * would step on whichever output happened to render next.
 *
 * `options.output` のフレームで `intervalMs` ごとに `callback` を実行します。
 * 時刻は出力ごと（各出力の時計はその出力のフレームの表示時刻）なので、
 * `options.output` は必須です。
 *
 * @example
 * ```ts
 * createPoll(16, (handle) => step(handle.nowMs), { output: window.output });
 * ```
 */
export function createPoll(
  intervalMs: number,
  callback: PollCallback,
  options: PollOptions,
): PollHandle {
  validateInterval(intervalMs);
  if (!options || options.output === undefined) {
    throw new Error("createPoll requires an output: createPoll(ms, callback, { output })");
  }
  const output = pollOutputResolver(options.output);
  if (activeBridge) {
    return activeBridge.registerPoll(
      intervalMs,
      callback,
      options.dirty ?? "runtime",
      output,
    );
  }
  return createDetachedPollHandle();
}

/**
 * Run `callback` every `intervalMs` on every enabled output, each on its own
 * clock. Outputs that appear later get their own poll; a poll whose output
 * goes away is cancelled. Cancelling the returned handle cancels all of them.
 *
 * 有効な全出力で、それぞれの時計に従って `intervalMs` ごとに `callback` を実行します。
 * 後から接続された出力にも poll が作られ、外れた出力の poll は取り消されます。
 * 返されたハンドルを cancel すると全て取り消されます。
 *
 * @example
 * ```ts
 * createPollForEachOutput(16, (handle, outputName) => {
 *   setTime(outputName, handle.nowMs / 1000);
 * });
 * ```
 */
export function createPollForEachOutput(
  intervalMs: number,
  callback: OutputPollCallback,
  options: OutputPollOptions = {},
): PollHandle {
  validateInterval(intervalMs);
  if (activeBridge) {
    return activeBridge.registerOutputPollGroup(
      intervalMs,
      callback,
      options.dirty ?? "runtime",
    );
  }
  return createDetachedPollHandle();
}

export function pollOutputResolver(output: PollOutput): () => string | undefined {
  if (typeof output === "string") {
    return () => output;
  }
  if (isSignal<string | undefined>(output)) {
    return () => output.peek();
  }
  return output as () => string | undefined;
}

function validateInterval(intervalMs: number): void {
  if (!Number.isFinite(intervalMs) || intervalMs <= 0) {
    throw new Error("createPoll interval must be a positive finite number");
  }
}

function createDetachedPollHandle(): PollHandle {
  let cancelled = false;
  let nowMs = 0;

  return {
    cancel() {
      cancelled = true;
    },
    get cancelled() {
      return cancelled;
    },
    get nowMs() {
      return nowMs;
    },
  };
}
