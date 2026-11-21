//! Statically compiled UI extensions. Engine command metadata remains authoritative for menus;
//! these descriptors only connect parameterless UI invocations to their dialogs.

use egui::Context;
use serde_json::Value;

use crate::CadApp;

#[cfg(feature = "gost")]
mod gost;
#[cfg(feature = "gost")]
pub(crate) mod models;

/// The dialog bindings provided by one compiled extension.
pub struct ExtensionDescriptor {
    pub id: &'static str,
    pub dialogs: &'static [DialogSpec],
}

/// Commands sharing a dialog and its renderer. Parameterized calls bypass UI routing.
pub struct DialogSpec {
    pub commands: &'static [&'static str],
    pub name: &'static str,
    pub render: fn(&mut CadApp, &Context, &str, &mut bool) -> Option<(&'static str, Value)>,
}

/// Enabled UI extensions, fixed at compile time; no runtime loading or registration.
pub fn compiled() -> &'static [ExtensionDescriptor] {
    &[
        #[cfg(feature = "gost")]
        gost::DESCRIPTOR,
        #[cfg(feature = "gost")]
        models::DESCRIPTOR,
    ]
}

pub(crate) fn command_dialog(command: &str) -> Option<&'static DialogSpec> {
    compiled().iter().flat_map(|extension| extension.dialogs).find(|dialog| dialog.commands.contains(&command))
}

pub(crate) fn named_dialog(name: &str) -> Option<&'static DialogSpec> {
    compiled().iter().flat_map(|extension| extension.dialogs).find(|dialog| dialog.name == name)
}
