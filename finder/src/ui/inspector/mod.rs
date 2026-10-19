//! Metadata Summary Inspector Dialog ("Get Info") using GTK 4 and libadwaita.

pub mod dialog;
pub mod general_page;
pub mod icon_custom;
pub mod more_info_page;
pub mod permissions_page;
pub mod theme;

pub use dialog::{GetInfoDialog, InspectorMode};
pub use general_page::{GeneralInspectorGroup, NameExtensionGroup};
pub use icon_custom::CustomIconBadge;
pub use more_info_page::{CommentsGroup, MoreInfoGroup, OriginGroup};
pub use permissions_page::{OpenWithGroup, SharingPermissionsGroup};
pub use theme::{INSPECTOR_CSS, install_inspector_theme};
