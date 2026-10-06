//! How the compositor paces frames per output (`COMPOSITOR.rendering.framePacing`).

use std::collections::HashMap;

/// Frame pacing of one output.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FramePacing {
    /// Render the next frame while the previous one waits for its vblank (render-ahead,
    /// triple buffering): a frame gets up to one extra refresh period, at the cost of one
    /// frame of latency while frames are continuous.
    #[default]
    Throughput,
    /// Render each frame only once the previous one is on screen.
    LowLatency,
}

/// The frame pacing the config chose, per output. Outputs it did not name use the
/// default; the compositor still turns render-ahead off where it would hurt (tearing,
/// fullscreen scanout) whatever this says.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FramePacingConfig {
    pub outputs: HashMap<String, FramePacing>,
}

impl FramePacingConfig {
    pub fn pacing(&self, output_name: &str) -> FramePacing {
        self.outputs.get(output_name).copied().unwrap_or_default()
    }

    pub fn allows_render_ahead(&self, output_name: &str) -> bool {
        self.pacing(output_name) == FramePacing::Throughput
    }
}
