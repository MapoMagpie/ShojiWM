# Output overlay regression checks

Run from the repository root inside `nix develop`:

```sh
npm run test:overlays   # SDK tests + typecheck of the examples and type fixture
cargo test -p shojiwm_lib backend::overlay
cargo test -p shoji_wm embedded_runtime_output_overlay
```

The SDK tests cover first-frame readiness, reactive uniforms, forwarding
output/placement/deadline/lifetime, rejected capture, disposal, legacy effect
configuration assignment and protection against awaited compositor callbacks
(an unrelated request in flight does not reject a detached task). The type
fixture includes the dissolve and persistent pixelation examples. The native
control tests cover deadlines, persistent readiness, owner isolation and runtime
teardown. The TypeScript runtime test covers initialization-time rejection,
awaited-callback rejection and detached capture timeout without a render loop.

Embedded test runtimes get a private `XDG_RUNTIME_DIR`, so no test touches the
user's config, IPC socket or running desktop session.
`run-native-tests.py` additionally runs a test binary in a fresh runtime
directory and checks that an inherited socket survives (needs python3).

The optional GPU probe needs surfaceless EGL/OpenGL ES and renders only
offscreen buffers:

```sh
LIBGL_ALWAYS_SOFTWARE=1 EGL_PLATFORM=surfaceless \
  cargo test -p shojiwm_lib output_overlay_gpu -- --ignored
```

It covers capture before readiness, replacement of a half-finished transition,
shader failure preserving the previous overlay, live opacity changes, idle cache
reuse, separate output slots, geometry/removal/lock cleanup and below-layers
placement at scale 1.5. It does not cover a full SDK-to-renderer round trip.
Offscreen tests cannot establish TTY/Winit presentation, hardware-driver timing,
monitor rotation/hotplug or the appearance of user shaders.
