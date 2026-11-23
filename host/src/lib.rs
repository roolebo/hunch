//! hunch gives agents the human's devices as files: hunch-host serves the
//! workspaces of the activities over 9P2000.L, and hunch-9p reaches them from
//! the shell. skills/hunch/TREE.md specifies both.

pub mod client;
pub mod device;
pub mod host;
pub mod native;
pub mod p9;
pub mod peer;
pub mod registry;
pub mod srv;
pub mod tree;
