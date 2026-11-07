// File: src/app/dialogs/mod.rs
// Purpose: Modal dialogs (Help, Settings, Image Info, Resize, Rename).

pub(crate) mod help;
pub(crate) mod image_info;
pub(crate) mod rename;
pub(crate) mod resize;
pub(crate) mod settings;

pub(crate) use help::draw_help;
pub(crate) use image_info::draw_image_info;
pub(crate) use rename::draw_rename_dialog;
pub(crate) use resize::draw_resize_dialog;
pub(crate) use settings::draw_settings;
