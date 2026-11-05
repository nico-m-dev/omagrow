//! Photos attached to a profile, and later to grows, plantings and observations.
//!
//! Files live next to the database under `media/`, never inside it. A row in the `media` table
//! carries the owner, the role and the stem of a file pair:
//!
//! ```text
//! media/<stem>-large.jpg   at most 2048 px on the longest side, for the detail view and analysis
//! media/<stem>-thumb.jpg   at most 320 px, for cards and lists
//! ```
//!
//! The original upload is not kept: the large copy is what the app shows, so the data directory
//! stays small and predictable. New files are written next to the old pair and renamed into place,
//! so a failed import leaves the previous photo intact.

use std::fs;
use std::path::{Path, PathBuf};

use gdk_pixbuf::{Colorspace, InterpType, Pixbuf};

use crate::store;

pub const LARGE_EDGE: i32 = 2048;
pub const THUMB_EDGE: i32 = 320;

/// One image row, without its pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Media {
    pub id: i64,
    pub owner_type: String,
    pub owner_id: i64,
    pub role: String,
    pub file_stem: String,
    pub width: i32,
    pub height: i32,
    pub bytes: i64,
}

/// What an import wrote, ready for the row.
#[derive(Clone, Debug, PartialEq)]
pub struct Imported {
    pub width: i32,
    pub height: i32,
    pub bytes: i64,
}

/// The name convention: `<owner_type>-<owner_id>-<role>`. Keep it filesystem-safe.
pub fn stem_for(owner_type: &str, owner_id: i64, role: &str) -> String {
    format!("{owner_type}-{owner_id}-{role}")
}

/// The folder for every image the app owns, or `None` when there is no data directory.
pub fn dir() -> Option<PathBuf> {
    store::data_dir().map(|dir| dir.join("media"))
}

pub fn large_path(stem: &str) -> Option<PathBuf> {
    Some(dir()?.join(format!("{stem}-large.jpg")))
}

pub fn thumb_path(stem: &str) -> Option<PathBuf> {
    Some(dir()?.join(format!("{stem}-thumb.jpg")))
}

/// The file a viewer or an agent should open: the large copy, falling back to the thumbnail when
/// the large one is missing.
pub fn detail_path(stem: &str) -> Option<PathBuf> {
    detail_path_in(&dir()?, stem)
}

/// [`detail_path`] against a directory, so a test can use a scratch folder.
pub fn detail_path_in(directory: &Path, stem: &str) -> Option<PathBuf> {
    [
        directory.join(format!("{stem}-large.jpg")),
        directory.join(format!("{stem}-thumb.jpg")),
    ]
    .into_iter()
    .find(|path| path.exists())
}

/// Imports into the app's media folder.
pub fn import(source: &Path, stem: &str) -> Result<Imported, String> {
    let directory = dir().ok_or("no data directory: set XDG_DATA_HOME or HOME")?;
    import_into(source, &directory, stem)
}

/// Imports into `directory`: decode at the target size, follow the EXIF orientation, flatten any
/// transparency onto white and write both copies.
///
/// Split from [`import`] so tests can use a scratch folder without touching the environment.
pub fn import_into(source: &Path, directory: &Path, stem: &str) -> Result<Imported, String> {
    fs::create_dir_all(directory)
        .map_err(|error| format!("could not create the media folder: {error}"))?;

    // Ask the loader for a reduced decode: a 48 MP photo never has to sit in memory at full size.
    // `from_file_at_scale` also enlarges, so the target is capped at the source dimensions.
    let Some((_, width, height)) = Pixbuf::file_info(source) else {
        return Err("could not read this image".to_string());
    };
    if width <= 0 || height <= 0 {
        return Err("this image has no size".to_string());
    }

    let longest = width.max(height);
    let scale = f64::from(LARGE_EDGE) / f64::from(longest.max(LARGE_EDGE));
    let target_width = ((f64::from(width) * scale).round() as i32).max(1);
    let target_height = ((f64::from(height) * scale).round() as i32).max(1);

    let pixbuf = Pixbuf::from_file_at_scale(source, target_width, target_height, true)
        .map_err(|error| format!("could not read this image: {}", error.message()))?;
    let pixbuf = match pixbuf.apply_embedded_orientation() {
        Some(oriented) => oriented,
        None => {
            eprintln!(
                "could not read the EXIF orientation of {}; using the image as it is",
                source.display()
            );
            pixbuf
        }
    };

    let pixbuf = flattened(&pixbuf);
    let large = scaled(&pixbuf, LARGE_EDGE, InterpType::Hyper);
    let thumb = scaled(&large, THUMB_EDGE, InterpType::Bilinear);

    let large_tmp = directory.join(format!("{stem}-large.jpg.tmp"));
    let thumb_tmp = directory.join(format!("{stem}-thumb.jpg.tmp"));

    if let Err(error) = save(&large, &large_tmp) {
        remove_files(&[&large_tmp, &thumb_tmp]);
        return Err(error);
    }
    if let Err(error) = save(&thumb, &thumb_tmp) {
        remove_files(&[&large_tmp, &thumb_tmp]);
        return Err(error);
    }

    let large_path = directory.join(format!("{stem}-large.jpg"));
    let thumb_path = directory.join(format!("{stem}-thumb.jpg"));

    install_pair(&large_tmp, &thumb_tmp, &large_path, &thumb_path)?;

    let bytes = [&large_path, &thumb_path]
        .into_iter()
        .filter_map(|path| fs::metadata(path).ok())
        .map(|meta| meta.len() as i64)
        .sum();

    Ok(Imported {
        width: large.width(),
        height: large.height(),
        bytes,
    })
}

/// Removes both files of a stem from the app's media folder.
pub fn remove(stem: &str) {
    let Some(directory) = dir() else {
        return;
    };
    remove_from(&directory, stem);
}

/// Removes both files of a stem from `directory`. A missing file is not an error.
pub fn remove_from(directory: &Path, stem: &str) {
    let files = [
        directory.join(format!("{stem}-large.jpg")),
        directory.join(format!("{stem}-thumb.jpg")),
    ];
    remove_files(&files);
}

/// Moves the written pair into place, keeping the old pair until both new files have landed.
fn install_pair(
    large_tmp: &Path,
    thumb_tmp: &Path,
    large: &Path,
    thumb: &Path,
) -> Result<(), String> {
    let large_backup = backup_path(large);
    let thumb_backup = backup_path(thumb);
    let large_existed = large.exists();
    let thumb_existed = thumb.exists();

    if large_existed {
        fs::rename(large, &large_backup)
            .map_err(|error| format!("could not put the photo in place: {error}"))?;
    }
    if thumb_existed && let Err(error) = fs::rename(thumb, &thumb_backup) {
        restore(&large_backup, large, large_existed);
        return Err(format!("could not put the photo in place: {error}"));
    }

    if let Err(error) = fs::rename(large_tmp, large) {
        restore(&large_backup, large, large_existed);
        restore(&thumb_backup, thumb, thumb_existed);
        remove_files(&[large_tmp, thumb_tmp]);
        return Err(format!("could not put the photo in place: {error}"));
    }
    if let Err(error) = fs::rename(thumb_tmp, thumb) {
        remove_files(&[large]);
        restore(&large_backup, large, large_existed);
        restore(&thumb_backup, thumb, thumb_existed);
        remove_files(&[thumb_tmp]);
        return Err(format!("could not put the thumbnail in place: {error}"));
    }

    remove_files(&[&large_backup, &thumb_backup]);
    Ok(())
}

fn backup_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("photo");
    path.with_file_name(format!("{name}.bak"))
}

fn restore(backup: &Path, path: &Path, existed: bool) {
    if existed {
        let _ = fs::rename(backup, path);
    } else {
        let _ = fs::remove_file(path);
    }
}

fn remove_files<T: AsRef<Path>>(files: &[T]) {
    for path in files {
        let path = path.as_ref();
        if let Err(error) = fs::remove_file(path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("could not remove {}: {error}", path.display());
        }
    }
}

/// JPEG has no alpha, and gdk-pixbuf turns an RGBA pixbuf into a greyscale JPEG. Flatten onto
/// white first so a transparent PNG keeps its colours.
fn flattened(pixbuf: &Pixbuf) -> Pixbuf {
    if !pixbuf.has_alpha() {
        return pixbuf.clone();
    }

    let Some(flat) = Pixbuf::new(Colorspace::Rgb, false, 8, pixbuf.width(), pixbuf.height()) else {
        return pixbuf.clone();
    };

    flat.fill(0xffffffff);
    pixbuf.composite(
        &flat,
        0,
        0,
        pixbuf.width(),
        pixbuf.height(),
        0.0,
        0.0,
        1.0,
        1.0,
        InterpType::Nearest,
        255,
    );

    flat
}

fn scaled(pixbuf: &Pixbuf, edge: i32, interpolation: InterpType) -> Pixbuf {
    let width = pixbuf.width();
    let height = pixbuf.height();
    let longest = width.max(height);

    if longest <= edge || longest == 0 {
        return pixbuf.clone();
    }

    let scale = f64::from(edge) / f64::from(longest);
    let target_width = ((f64::from(width) * scale).round() as i32).max(1);
    let target_height = ((f64::from(height) * scale).round() as i32).max(1);

    pixbuf
        .scale_simple(target_width, target_height, interpolation)
        .unwrap_or_else(|| pixbuf.clone())
}

fn save(pixbuf: &Pixbuf, path: &Path) -> Result<(), String> {
    pixbuf
        .savev(path, "jpeg", &[("quality", "90")])
        .map_err(|error| format!("could not save the photo: {}", error.message()))
}

#[cfg(test)]
mod tests {
    use super::{LARGE_EDGE, THUMB_EDGE, detail_path_in, import_into, install_pair, remove_from};
    use gdk_pixbuf::{Colorspace, Pixbuf};

    fn scratch(name: &str) -> std::path::PathBuf {
        let directory =
            std::env::temp_dir().join(format!("omagrow-media-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    fn source_image(path: &std::path::Path, width: i32, height: i32, alpha: bool) {
        let pixbuf = Pixbuf::new(Colorspace::Rgb, alpha, 8, width, height).expect("a pixbuf");
        pixbuf.fill(0xcc3311ff);
        pixbuf.savev(path, "png", &[]).unwrap();
    }

    #[test]
    fn writes_a_large_copy_and_a_thumbnail() {
        let directory = scratch("large");
        let source = directory.join("upload.jpg");
        source_image(&source, 2400, 1200, false);

        let imported = import_into(&source, &directory, "profile-1-cover").unwrap();
        assert_eq!(imported.width, LARGE_EDGE);
        assert_eq!(imported.height, LARGE_EDGE / 2);
        assert!(imported.bytes > 0);

        let large = directory.join("profile-1-cover-large.jpg");
        let thumb = directory.join("profile-1-cover-thumb.jpg");
        assert!(large.exists());
        assert!(thumb.exists());

        let thumb = Pixbuf::from_file(&thumb).unwrap();
        assert_eq!(thumb.width(), THUMB_EDGE);
        assert_eq!(thumb.height(), THUMB_EDGE / 2);

        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn the_detail_view_prefers_the_large_copy() {
        let directory = scratch("detail");
        let large = directory.join("x-large.jpg");
        let thumb = directory.join("x-thumb.jpg");
        std::fs::write(&large, "large").unwrap();
        std::fs::write(&thumb, "thumb").unwrap();

        assert_eq!(
            detail_path_in(&directory, "x").as_deref(),
            Some(large.as_path())
        );
        std::fs::remove_file(&large).unwrap();
        assert_eq!(
            detail_path_in(&directory, "x").as_deref(),
            Some(thumb.as_path())
        );

        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_small_image_is_not_blown_up() {
        let directory = scratch("small");
        let source = directory.join("upload.jpg");
        source_image(&source, 200, 100, false);

        let imported = import_into(&source, &directory, "profile-2-cover").unwrap();
        assert_eq!(imported.width, 200);
        assert_eq!(imported.height, 100);

        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_transparent_png_keeps_its_colours() {
        let directory = scratch("alpha");
        let source = directory.join("upload.png");
        source_image(&source, 400, 300, true);

        import_into(&source, &directory, "profile-3-cover").unwrap();
        let large = Pixbuf::from_file(directory.join("profile-3-cover-large.jpg")).unwrap();
        let pixels = large.read_pixel_bytes();
        let red = pixels[0];

        assert!(
            red > 150,
            "a transparent PNG must not become a greyscale JPEG (red channel {red})"
        );

        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn replacing_a_cover_keeps_one_pair() {
        let directory = scratch("replace");
        let source = directory.join("upload.jpg");
        source_image(&source, 800, 600, false);

        import_into(&source, &directory, "profile-4-cover").unwrap();
        import_into(&source, &directory, "profile-4-cover").unwrap();

        let files: Vec<_> = std::fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("profile-4-cover"))
            .collect();
        assert_eq!(files.len(), 2, "{files:?}");

        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn removing_a_stem_takes_both_files() {
        let directory = scratch("remove");
        let source = directory.join("upload.jpg");
        source_image(&source, 500, 500, false);

        import_into(&source, &directory, "profile-5-cover").unwrap();
        remove_from(&directory, "profile-5-cover");
        remove_from(&directory, "profile-5-cover");

        let remaining: Vec<_> = std::fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name != "upload.jpg")
            .collect();
        assert!(remaining.is_empty(), "{remaining:?}");

        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn installing_a_pair_replaces_both_files() {
        let directory = scratch("install");
        let large = directory.join("x-large.jpg");
        let thumb = directory.join("x-thumb.jpg");
        let large_tmp = directory.join("x-large.jpg.tmp");
        let thumb_tmp = directory.join("x-thumb.jpg.tmp");
        std::fs::write(&large, "old-large").unwrap();
        std::fs::write(&thumb, "old-thumb").unwrap();
        std::fs::write(&large_tmp, "new-large").unwrap();
        std::fs::write(&thumb_tmp, "new-thumb").unwrap();

        install_pair(&large_tmp, &thumb_tmp, &large, &thumb).unwrap();

        assert_eq!(std::fs::read_to_string(&large).unwrap(), "new-large");
        assert_eq!(std::fs::read_to_string(&thumb).unwrap(), "new-thumb");
        assert!(!directory.join("x-large.jpg.bak").exists());
        assert!(!directory.join("x-thumb.jpg.bak").exists());

        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_failed_install_restores_the_old_pair() {
        let directory = scratch("install-fail");
        let large = directory.join("x-large.jpg");
        let thumb = directory.join("x-thumb.jpg");
        let large_tmp = directory.join("x-large.jpg.tmp");
        let thumb_tmp = directory.join("x-thumb.jpg.tmp");
        std::fs::write(&large, "old-large").unwrap();
        std::fs::write(&thumb, "old-thumb").unwrap();
        std::fs::write(&large_tmp, "new-large").unwrap();
        // No thumb tmp: the second rename fails after the large one has landed.

        assert!(install_pair(&large_tmp, &thumb_tmp, &large, &thumb).is_err());

        assert_eq!(std::fs::read_to_string(&large).unwrap(), "old-large");
        assert_eq!(std::fs::read_to_string(&thumb).unwrap(), "old-thumb");
        assert!(large.exists());
        assert!(!directory.join("x-large.jpg.bak").exists());
        assert!(!directory.join("x-thumb.jpg.bak").exists());

        std::fs::remove_dir_all(&directory).unwrap();
    }
}
