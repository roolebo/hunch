//! The devices hunch gives agents: PROTOCOL.md's and BROWSER.md's Files. A
//! device's files never refer to other devices; hunch-host assembles them.

mod browser;
mod buffers;
mod editor;

pub use browser::browser;
pub use buffers::Buffers;
pub use editor::editor;
