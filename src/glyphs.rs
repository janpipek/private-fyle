//! Which glyph from GPUI Kit's default bundle draws each [`FileIcon`], shared
//! by the list and the icon grid.

use gpui_kit::assets::IconName as Lucide;

use crate::icons::FileIcon;

/// The glyph drawing each kind of [`FileIcon`]. Only icons from GPUI Kit's
/// default bundle are used (see the test), so no icon assets are vendored.
pub fn glyph(icon: FileIcon) -> Lucide {
    match icon {
        FileIcon::ParentDir => Lucide::ArrowUp,
        FileIcon::Folder => Lucide::Folder,
        FileIcon::FolderOpen => Lucide::FolderOpen,
        FileIcon::FolderGit => Lucide::Github,
        FileIcon::Code | FileIcon::Script => Lucide::SquareTerminal,
        FileIcon::Config => Lucide::Settings,
        FileIcon::Data | FileIcon::Text => Lucide::FileText,
        FileIcon::Image => Lucide::Palette,
        FileIcon::Audio | FileIcon::Video => Lucide::Play,
        FileIcon::Archive => Lucide::Inbox,
        FileIcon::Spreadsheet => Lucide::ChartPie,
        FileIcon::Font => Lucide::CaseSensitive,
        FileIcon::Diff => Lucide::Replace,
        FileIcon::File | FileIcon::Lock | FileIcon::Key => Lucide::File,
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: gpui's glob export would shadow `#[test]`.
    use super::glyph;
    use crate::icons::FileIcon;
    use gpui_kit::AssetSource as _;
    use gpui_kit::assets::IconName as Lucide;

    #[test]
    fn every_glyph_is_in_the_default_bundle() {
        for icon in FileIcon::ALL {
            let path = glyph(icon).path();
            let svg = gpui_kit::assets::Assets.load(&path).unwrap();
            assert!(svg.is_some(), "{icon:?}: {path} is not bundled");
        }
        for icon in [
            Lucide::Redo2,
            Lucide::Ban,
            Lucide::Map,
            Lucide::SquareTerminal,
            Lucide::Copy,
            Lucide::ChevronRight,
            Lucide::Check,
        ] {
            let path = icon.path();
            let svg = gpui_kit::assets::Assets.load(&path).unwrap();
            assert!(svg.is_some(), "header icon {icon:?}: {path} is not bundled");
        }
    }
}
