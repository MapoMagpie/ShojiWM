//! The default ShojiWM binary: the compositor (`shojiwm_lib`) with the
//! embedded TypeScript config runtime. Binaries for other config languages
//! look the same with their own launcher.

fn main() -> std::process::ExitCode {
    shojiwm_lib::run(shoji_wm::TypeScriptLauncher)
}
