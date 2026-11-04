// Badi's reply protocol. The model puts visual tags in its reply ([POINT], [SHAPE], [WRITE]…);
// `tags` parses them and `steps` cuts the stream into steps that are shown and spoken together.

pub mod claude;
pub mod persona;
pub mod prompts;
pub mod steps;
pub mod tags;

pub use steps::{Step, StepSplitter};
pub use tags::{Shot, Tag, extract_tags, parse_tag, to_global};
