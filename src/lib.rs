pub mod option;
pub mod schema;
pub mod state;

pub use option::{NixOption, OptionType, OptionValue};
pub use schema::SchemaStore;
pub use state::ConfigState;

pub mod eval;
pub mod parser;
pub mod writer;

pub use eval::NixEvaluator;
pub use parser::FlakeParser;
pub use writer::FlakeWriter;

pub mod app;
pub mod input;
pub mod render;
pub mod widgets;

pub use app::{App, AppMode, Screen};
