//! Generic immutable replacement table, independent of compression features.
pub use rfvp_plugin_api::StringReplacements;

#[cfg(feature = "old_school")]
pub(crate) type SharedStrings = alloc::rc::Rc<StringReplacements>;
#[cfg(not(feature = "old_school"))]
pub(crate) type SharedStrings = alloc::sync::Arc<StringReplacements>;
