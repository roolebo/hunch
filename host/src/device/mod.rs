//! The devices hunch gives agents: PROTOCOL.md's Files. A device's files
//! never refer to other devices; hunch-host assembles them.

mod buffers;
mod editor;

pub use buffers::Buffers;
pub use editor::editor;
