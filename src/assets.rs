//! Application assets: a few extra Lucide icons (ISC, see
//! `assets/icons/LICENSE`) on top of GPUI Kit's default icon bundle, so the
//! binary does not embed the whole Lucide set.

use std::borrow::Cow;

use gpui_kit::*;

const EXTRA: &[(&str, &[u8])] = &[
    (
        "icons/file-archive.svg",
        include_bytes!("../assets/icons/file-archive.svg"),
    ),
    (
        "icons/file-braces.svg",
        include_bytes!("../assets/icons/file-braces.svg"),
    ),
    (
        "icons/file-code.svg",
        include_bytes!("../assets/icons/file-code.svg"),
    ),
    (
        "icons/file-cog.svg",
        include_bytes!("../assets/icons/file-cog.svg"),
    ),
    (
        "icons/file-diff.svg",
        include_bytes!("../assets/icons/file-diff.svg"),
    ),
    (
        "icons/file-image.svg",
        include_bytes!("../assets/icons/file-image.svg"),
    ),
    (
        "icons/file-key.svg",
        include_bytes!("../assets/icons/file-key.svg"),
    ),
    (
        "icons/file-lock.svg",
        include_bytes!("../assets/icons/file-lock.svg"),
    ),
    (
        "icons/file-music.svg",
        include_bytes!("../assets/icons/file-music.svg"),
    ),
    (
        "icons/file-spreadsheet.svg",
        include_bytes!("../assets/icons/file-spreadsheet.svg"),
    ),
    (
        "icons/file-terminal.svg",
        include_bytes!("../assets/icons/file-terminal.svg"),
    ),
    (
        "icons/file-type.svg",
        include_bytes!("../assets/icons/file-type.svg"),
    ),
    (
        "icons/file-video-camera.svg",
        include_bytes!("../assets/icons/file-video-camera.svg"),
    ),
    (
        "icons/folder-git-2.svg",
        include_bytes!("../assets/icons/folder-git-2.svg"),
    ),
    (
        "icons/folder-up.svg",
        include_bytes!("../assets/icons/folder-up.svg"),
    ),
];

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match EXTRA.iter().find(|(p, _)| *p == path) {
            Some((_, data)) => Ok(Some(Cow::Borrowed(data))),
            None => gpui_kit::assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut items = gpui_kit::assets::Assets.list(path)?;
        items.extend(
            EXTRA
                .iter()
                .filter(|(p, _)| p.starts_with(path))
                .map(|(p, _)| SharedString::from(*p)),
        );
        Ok(items)
    }
}
