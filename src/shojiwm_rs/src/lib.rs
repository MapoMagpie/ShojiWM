//! Write a ShojiWM config in Rust.
//!
//! A Rust config is an ordinary crate whose `main` hands a [`RustLauncher`] to
//! [`run`]. The config itself is a [`ConfigRuntime`]: it is called on the
//! compositor thread, answers [`RuntimeRequest`]s in place (no serialization,
//! no extra thread) and sends config deltas through the [`RuntimeHost`] it is
//! built with.
//!
//! ```no_run
//! use shojiwm_rs::*;
//!
//! struct MyConfig {
//!     host: RuntimeHost,
//! }
//!
//! impl ConfigRuntime for MyConfig {
//!     fn request(
//!         &mut self,
//!         _now_ms: f64,
//!         _request: RuntimeRequest<'_>,
//!     ) -> Result<RuntimeReply, RuntimeError> {
//!         // Anything not handled falls back to the compositor's defaults.
//!         Ok(RuntimeReply::Unhandled)
//!     }
//! }
//!
//! fn main() -> std::process::ExitCode {
//!     run(RustLauncher::new(|context| MyConfig { host: context.host }))
//! }
//! ```

pub use shojiwm_lib::run;
pub use shojiwm_lib::runtime_api::{self, *};
/// Data types a config reads (snapshots) and builds (decoration trees,
/// effect configs).
pub use shojiwm_lib::ssd;

/// [`RuntimeLauncher`] for a config compiled into the binary.
pub struct RustLauncher<F> {
    name: &'static str,
    factory: F,
}

impl<F, R> RustLauncher<F>
where
    F: Fn(LaunchContext) -> R,
    R: ConfigRuntime + 'static,
{
    /// `factory` builds the config once the command line is parsed. Keep the
    /// context's `host` to send config deltas later.
    pub fn new(factory: F) -> Self {
        Self {
            name: "rust",
            factory,
        }
    }

    /// Name shown in logs and `--help` (default `"rust"`).
    pub fn with_name(mut self, name: &'static str) -> Self {
        self.name = name;
        self
    }
}

impl<F, R> RuntimeLauncher for RustLauncher<F>
where
    F: Fn(LaunchContext) -> R,
    R: ConfigRuntime + 'static,
{
    fn name(&self) -> &'static str {
        self.name
    }

    fn launch(&self, context: LaunchContext) -> Box<dyn ConfigRuntime> {
        Box::new((self.factory)(context))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct BindOnEnable {
        host: RuntimeHost,
    }

    impl ConfigRuntime for BindOnEnable {
        fn enable(&mut self) -> Result<(), RuntimeError> {
            self.host.send(HostMessage::ProcessActions(Vec::new()));
            Ok(())
        }

        fn request(
            &mut self,
            _now_ms: f64,
            request: RuntimeRequest<'_>,
        ) -> Result<RuntimeReply, RuntimeError> {
            Ok(match request {
                RuntimeRequest::Input(InputRequest::KeyBinding { .. }) => {
                    RuntimeReply::KeyBinding(ssd::DecorationKeyBindingInvocation {
                        invoked: true,
                        ..Default::default()
                    })
                }
                _ => RuntimeReply::Unhandled,
            })
        }
    }

    #[test]
    fn rust_runtime_answers_in_place_and_falls_back_otherwise() {
        let launcher = RustLauncher::new(|context| BindOnEnable { host: context.host });
        let args = cli::CommonArgs::parse(&[], launcher.extra_args());
        let host = RuntimeHost::detached();
        let mut runtime = RuntimeBoot::new(Box::new(launcher), &args).launch(host.clone());
        assert_eq!(runtime.name(), "rust");

        runtime.enable().unwrap();
        assert!(matches!(host.pop(), Some(HostMessage::ProcessActions(_))));
        assert!(runtime.invoke_key_binding("any", 0).unwrap().invoked);
        assert!(runtime.scheduler_tick(0.0).unwrap().next_poll_in_ms.is_none());
    }
}
