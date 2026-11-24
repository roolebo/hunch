//! The devices hunch gives agents: the Files of PROTOCOL.md, BROWSER.md
//! and INFERENCE.md. A device's files never refer to other devices;
//! hunch-host assembles them.

mod browser;
mod buffers;
mod editor;
mod inference;

pub use browser::browser;
pub use buffers::Buffers;
pub use editor::editor;
pub use inference::{Inference, Keys, OsKeys};
