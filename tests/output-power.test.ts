import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import { COMPOSITOR, type OutputPower } from "../packages/shoji_wm/src/index";

type Call = [power: string, output: string, wakeOnInput: boolean];
const runtime = globalThis as { __SHOJI_OUTPUT_POWER__?: (...args: Call) => void };

function installNative(): Call[] {
  const calls: Call[] = [];
  runtime.__SHOJI_OUTPUT_POWER__ = (...args) => { calls.push(args); };
  return calls;
}

afterEach(() => { delete runtime.__SHOJI_OUTPUT_POWER__; });

test("setPower targets every output unless one is named", () => {
  const calls = installNative();
  COMPOSITOR.output.setPower("off");
  COMPOSITOR.output.setPower("on", { output: "DP-1" });
  assert.deepEqual(calls, [["off", "", false], ["on", "DP-1", false]]);
});

test("wakeOnInput is forwarded and defaults to false", () => {
  const calls = installNative();
  COMPOSITOR.output.setPower("off", { wakeOnInput: true });
  COMPOSITOR.output.setPower("toggle", { output: "eDP-1" });
  assert.deepEqual(calls, [["off", "", true], ["toggle", "eDP-1", false]]);
});

test("an unknown power mode throws instead of reaching the compositor", () => {
  const calls = installNative();
  assert.throws(() => COMPOSITOR.output.setPower("standby" as OutputPower), TypeError);
  assert.deepEqual(calls, []);
});

test("outside the ShojiWM runtime setPower does nothing", () => {
  assert.doesNotThrow(() => COMPOSITOR.output.setPower("off"));
});
