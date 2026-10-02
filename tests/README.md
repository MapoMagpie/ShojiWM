# Output overlay regression checks

These checks are carried over from the local implementation and adapted to the
compositor/runtime crate split. They have not been run on this PR branch.

From the repository root, with the existing Node development dependencies:

```sh
node --import tsx tests/output-overlay.test.ts
npx tsc -p packages/shoji_wm/tsconfig.json
npx tsc -p packages/config/tsconfig.json
npx tsc -p tests/tsconfig.overlays.json
python3 tests/run-native-tests.py cargo test -p shojiwm_lib backend::overlay::tests -- --test-threads=1
python3 tests/run-native-tests.py cargo test -p shoji_wm embedded_runtime_output_overlay -- --test-threads=1
```

The seven SDK tests cover first-frame readiness, reactive uniforms, forwarding
output/placement/deadline/lifetime, rejected capture, disposal, legacy effect
configuration assignment and protection against awaited compositor callbacks.
The type fixture includes the dissolve and persistent pixelation examples.
Three native control tests cover deadlines, persistent readiness, owner isolation
and runtime teardown. The TypeScript runtime test covers initialization-time
rejection, awaited-callback rejection and detached capture timeout without a
render loop.

The optional native GPU probe requires surfaceless EGL/OpenGL ES and only renders
offscreen buffers:

```sh
LIBGL_ALWAYS_SOFTWARE=1 EGL_PLATFORM=surfaceless python3 tests/run-native-tests.py cargo test -p shojiwm_lib output_overlay_gpu -- --ignored --nocapture --test-threads=1
```

It covers capture before readiness, replacement of a half-finished transition,
shader failure preserving the previous overlay, live opacity changes, idle cache
reuse, separate output slots, geometry/removal/lock cleanup and below-layers
placement at scale 1.5. It does not cover a successful SDK-to-renderer round trip;
that part of the old single-crate probe still needs adapting across the new crate
boundary.

Use the private IPC runner for native checks. It removes inherited display/wake
settings and verifies that its sentinel socket survives. Embedded test runtimes
also receive separate private IPC directories. No test needs the user's config
or a running desktop session.

Build and real-session validation remain pending. Offscreen tests cannot establish
TTY/Winit presentation, hardware-driver timing, monitor rotation/hotplug or the
appearance of user shaders.
