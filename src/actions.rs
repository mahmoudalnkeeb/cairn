//! Quick actions on a selection: copy paths, copy as Markdown links, zip,
//! and copy to a folder. Zipping runs in-process on a worker thread and
//! posts the result back to the host window.

use std::fs::File;
use std::io;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::*;
use windows::Win32::Storage::FileSystem::FileTimeToLocalFileTime;
use windows::Win32::System::Com::*;
use windows::Win32::System::Time::FileTimeToSystemTime;
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::core::*;
use zip::write::SimpleFileOptions;
use zip::{DateTime, ZipWriter};

use crate::shelf::{self, Kind, ShelfItem};

/// Posted to the host window when a zip finishes: wParam is 1 on success.
pub const WM_ZIP_DONE: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 2;

/// One line per file path or link; text items are left out.
pub fn paths_text(kinds: &[&Kind]) -> String {
    kinds
        .iter()
        .filter_map(|k| match k {
            Kind::File(p) => Some(p.to_string_lossy().into_owned()),
            Kind::Link(l) => Some(l.clone()),
            Kind::Text(_) => None,
        })
        .collect::<Vec<_>>()
        .join("\r\n")
}

/// `file:///` URL for a path, with anything outside the safe set escaped.
pub fn file_url(path: &Path) -> String {
    let raw = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file:///");
    for b in raw.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// One Markdown link per file or link, labelled with its tile name (a page
/// title when there is one), as a list when there are several.
pub fn markdown(items: &[&ShelfItem]) -> String {
    let links: Vec<String> = items
        .iter()
        .filter_map(|item| {
            let label = shelf::label(item).replace('[', "\\[").replace(']', "\\]");
            match &item.kind {
                Kind::File(p) => Some(format!("[{label}]({})", file_url(p))),
                Kind::Link(l) if l.starts_with("www.") => Some(format!("[{label}](https://{l})")),
                Kind::Link(l) => Some(format!("[{label}]({l})")),
                Kind::Text(_) => None,
            }
        })
        .collect();
    match links.len() {
        1 => links.into_iter().next().unwrap(),
        _ => links.iter().map(|l| format!("- {l}")).collect::<Vec<_>>().join("\r\n"),
    }
}

/// Files and folders in a selection that still exist.
pub fn existing_files(kinds: &[&Kind]) -> Vec<PathBuf> {
    kinds
        .iter()
        .filter_map(|k| if let Kind::File(p) = k { Some(p.clone()) } else { None })
        .filter(|p| p.exists())
        .collect()
}

/// Asks for a folder with the standard picker.
pub fn pick_folder(owner: HWND) -> Option<PathBuf> {
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let opts = dialog.GetOptions().ok()?;
        dialog.SetOptions(opts | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM).ok()?;
        dialog.SetTitle(w!("Copy to folder")).ok()?;
        dialog.SetOkButtonLabel(w!("Copy here")).ok()?;
        dialog.Show(owner).ok()?;
        item_path(&dialog.GetResult().ok()?)
    }
}

/// Asks where a missing file went, starting in its old folder, or the
/// nearest one above it that still exists.
pub fn pick_file(owner: HWND, old: &Path) -> Option<PathBuf> {
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        dialog.SetTitle(w!("Find file")).ok()?;
        if let Some(name) = old.file_name() {
            let _ = dialog.SetFileName(&HSTRING::from(name));
        }
        if let Some(dir) = old.ancestors().skip(1).find(|d| d.is_dir())
            && let Ok(folder) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(dir.as_os_str()), None)
        {
            let _ = dialog.SetFolder(&folder);
        }
        dialog.Show(owner).ok()?;
        item_path(&dialog.GetResult().ok()?)
    }
}

fn item_path(item: &IShellItem) -> Option<PathBuf> {
    unsafe {
        let p = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = PathBuf::from(p.to_string().ok()?);
        CoTaskMemFree(Some(p.0 as *const _));
        Some(path)
    }
}

/// Copies files into a folder with the shell's own progress and conflict
/// dialogs. Returns false if the user cancelled or it failed.
pub fn copy_to(owner: HWND, files: &[PathBuf], folder: &Path) -> bool {
    unsafe {
        let Ok(op) = CoCreateInstance::<_, IFileOperation>(&FileOperation, None, CLSCTX_ALL) else { return false };
        let _ = op.SetOwnerWindow(owner);
        let _ = op.SetOperationFlags(FOF_NOCONFIRMMKDIR | FOFX_ADDUNDORECORD);
        let Ok(dest) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(folder.as_os_str()), None) else { return false };
        for f in files {
            if let Ok(src) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(f.as_os_str()), None) {
                let _ = op.CopyItem(&src, &dest, None, None);
            }
        }
        op.PerformOperations().is_ok() && !op.GetAnyOperationsAborted().is_ok_and(|a| a.as_bool())
    }
}

/// Asks where to save a zip, starting in the first file's folder.
pub fn pick_zip(owner: HWND, near: &Path) -> Option<PathBuf> {
    unsafe {
        let dialog: IFileSaveDialog = CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let filter = [COMDLG_FILTERSPEC { pszName: w!("Zip archive"), pszSpec: w!("*.zip") }];
        dialog.SetFileTypes(&filter).ok()?;
        dialog.SetDefaultExtension(w!("zip")).ok()?;
        dialog.SetFileName(w!("Cairn items.zip")).ok()?;
        dialog.SetTitle(w!("Save as zip")).ok()?;
        if let Some(dir) = near.parent()
            && let Ok(folder) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(dir.as_os_str()), None)
        {
            let _ = dialog.SetFolder(&folder);
        }
        dialog.Show(owner).ok()?;
        item_path(&dialog.GetResult().ok()?)
    }
}

/// Zips files and folders into `out` on a worker thread, then posts
/// WM_ZIP_DONE to `notify`.
pub fn zip(files: Vec<PathBuf>, out: PathBuf, notify: HWND) {
    let target = notify.0 as isize;
    std::thread::spawn(move || {
        let ok = write_zip(&files, &out).is_ok();
        if !ok {
            let _ = std::fs::remove_file(&out);
        }
        unsafe {
            let _ = PostMessageW(HWND(target as *mut _), WM_ZIP_DONE, WPARAM(ok as usize), LPARAM(0));
        }
    });
}

/// Each item is added under its own name, so the zip holds names, not full
/// paths. Folders bring everything inside them.
fn write_zip(files: &[PathBuf], out: &Path) -> zip::result::ZipResult<()> {
    let mut zip = ZipWriter::new(File::create(out)?);
    for f in files {
        if let Some(name) = f.file_name() {
            add(&mut zip, f, &name.to_string_lossy())?;
        }
    }
    zip.finish()?;
    Ok(())
}

fn add(zip: &mut ZipWriter<File>, path: &Path, name: &str) -> zip::result::ZipResult<()> {
    let meta = std::fs::metadata(path)?;
    let mut options = SimpleFileOptions::default().large_file(meta.len() >= u32::MAX as u64);
    if let Some(time) = zip_time(meta.last_write_time()) {
        options = options.last_modified_time(time);
    }
    if meta.is_dir() {
        zip.add_directory(format!("{name}/"), options)?;
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            add(zip, &entry.path(), &format!("{name}/{}", entry.file_name().to_string_lossy()))?;
        }
    } else {
        zip.start_file(name, options)?;
        io::copy(&mut File::open(path)?, zip)?;
    }
    Ok(())
}

/// Zips store local time. None outside the years a zip can hold (1980 to 2107).
fn zip_time(filetime: u64) -> Option<DateTime> {
    let utc = FILETIME { dwLowDateTime: filetime as u32, dwHighDateTime: (filetime >> 32) as u32 };
    let (mut local, mut t) = (FILETIME::default(), SYSTEMTIME::default());
    unsafe {
        FileTimeToLocalFileTime(&utc, &mut local).ok()?;
        FileTimeToSystemTime(&local, &mut t).ok()?;
    }
    DateTime::from_date_and_time(t.wYear, t.wMonth as u8, t.wDay as u8, t.wHour as u8, t.wMinute as u8, t.wSecond as u8).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_paths_and_links_but_not_text() {
        let (f, t, l) = (Kind::File("C:\\a b\\x.txt".into()), Kind::Text("note".into()), Kind::Link("https://e.com".into()));
        assert_eq!(paths_text(&[&f, &t, &l]), "C:\\a b\\x.txt\r\nhttps://e.com");
    }

    #[test]
    fn markdown_links_escape_paths_and_labels() {
        let item = |kind, title: Option<&str>| ShelfItem { id: 0, kind, icon: None, extra: shelf::Extra { title: title.map(Into::into), ..Default::default() } };
        let f = item(Kind::File("C:\\My Docs\\[draft] #1.md".into()), None);
        assert_eq!(markdown(&[&f]), "[\\[draft\\] #1.md](file:///C:/My%20Docs/%5Bdraft%5D%20%231.md)");
        let (a, b) = (item(Kind::Link("www.e.com/x".into()), None), item(Kind::Link("https://e.com".into()), Some("E [home]")));
        assert_eq!(markdown(&[&a, &b]), "- [e.com/x](https://www.e.com/x)\r\n- [E \\[home\\]](https://e.com)");
    }

    #[test]
    fn zips_files_and_folders_from_different_folders() {
        let dir = std::env::temp_dir().join("cairn-zip-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("one")).unwrap();
        std::fs::create_dir_all(dir.join("two").join("inner")).unwrap();
        let (a, b) = (dir.join("one").join("a.txt"), dir.join("two").join("inner").join("b.txt"));
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        let out = dir.join("out.zip");
        write_zip(&[a, dir.join("two")], &out).unwrap();
        let mut zip = zip::ZipArchive::new(File::open(&out).unwrap()).unwrap();
        let mut names: Vec<&str> = zip.file_names().collect();
        names.sort();
        assert_eq!(names, ["a.txt", "two/", "two/inner/", "two/inner/b.txt"]);
        let mut text = String::new();
        io::Read::read_to_string(&mut zip.by_name("two/inner/b.txt").unwrap(), &mut text).unwrap();
        assert_eq!(text, "b");
        // Keeps the file's own date, not the 1980 default.
        assert!(zip.by_name("a.txt").unwrap().last_modified().is_some_and(|t| t.year() >= 2026));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
