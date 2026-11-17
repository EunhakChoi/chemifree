//! A read-only MCP server for a running Chemifree API server.
//!
//! The adapter is a client of the API that `chemifree-api` serves on loopback. It does not open
//! documents itself, and it does not build an application, take the server lock or start a server.
//! That is deliberate (P82): an agent's edits made through MCP must be visible in the web editor that
//! the user has open, and the only place where those unsaved edits exist is the running server's
//! session. A process of its own would hold a separate session that the editor never sees, and it
//! would need the data directory's server lock, which the running server already holds.
//!
//! The modules are:
//!
//! - [`discovery`] finds the server's address: an explicit `--server-url`, or the address that the
//!   running server recorded in its data directory.
//! - [`client`] makes the API calls: the session header and token, the operation envelope, request
//!   ids and the reply size limit.
//! - [`tools`] offers the read tools and the edit tools (create, transaction, undo, redo, save, close),
//!   and maps their replies to MCP results.

pub mod client;
pub mod discovery;
pub mod tools;
