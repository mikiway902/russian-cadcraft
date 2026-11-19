//! Product policy is explicit; embedding the drafting engine never changes its native defaults.
//!
//! Application hosts choose this policy once at startup. Opening a document and the low-level
//! `Session::new_drawing` API always preserve native units, layouts and object identities.

use crate::{Session, cmd};

/// Start-tab session with CADCraftGOST defaults for subsequently created documents.
pub fn session() -> Session {
    let mut session = Session::empty();
    session.extension_defaults = true;
    session.settings.measureinit = true;
    session
}

/// Product session with a clean, untitled drawing in model space (CLI and MCP).
pub fn new_session() -> Session {
    let mut session = session();
    new_drawing(&mut session, true);
    session
}

/// Create an untitled drawing using the host's selected policy, not the library's constructors.
pub fn new_drawing(session: &mut Session, metric: bool) -> usize {
    if session.extension_defaults && metric { session.new_document(cmd::gost::new_drawing()) } else { session.new_drawing(metric) }
}
