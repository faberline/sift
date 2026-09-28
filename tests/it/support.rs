//! Helpers shared by the `it` integration-test modules.

/// The libtest name of `function` in the module whose `module_path!()` is
/// `module_path`, for re-running exactly that case with `--exact`.
#[cfg(unix)]
pub fn test_name(module_path: &str, function: &str) -> String {
    let module = module_path
        .split_once("::")
        .map_or("", |(_crate, module)| module);
    format!("{module}::{function}")
}
