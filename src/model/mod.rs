mod highlight;
mod parse;
mod plugins;
mod project;
mod rust_symbols;
mod types;

pub use highlight::colorized_segments_with_calls;
pub use parse::find_function_span;
pub use project::ProjectModel;
pub use types::{DefinitionLocation, FunctionCall, ParsedFile};
