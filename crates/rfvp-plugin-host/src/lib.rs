//! Host-owned sources and optional interpreter for desktop preparation.
pub use rfvp_plugin_sdk as abi;
pub mod identity;
#[cfg(feature = "wasmi-runtime")]
pub mod runtime;
pub mod source;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Manifest,
    Inputs,
    Metadata,
    Module,
    Execute,
    Plan,
    Compose,
}

#[derive(Debug)]
pub struct PluginError {
    pub plugin: String,
    pub stage: Stage,
    pub cause: anyhow::Error,
}
impl std::fmt::Display for PluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "plugin {} [{:?}]: {:#}",
            self.plugin, self.stage, self.cause
        )
    }
}
impl std::error::Error for PluginError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.cause.as_ref())
    }
}
pub fn at<T>(plugin: &str, stage: Stage, result: anyhow::Result<T>) -> Result<T, PluginError> {
    result.map_err(|cause| match cause.downcast::<PluginError>() {
        Ok(error) => error,
        Err(cause) => PluginError {
            plugin: plugin.into(),
            stage,
            cause,
        },
    })
}
