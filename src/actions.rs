//! Quick actions on a selection: copy paths, copy as Markdown links, zip,
//! and copy to a folder. Zipping uses the tar.exe that ships with Windows 10
//! and 11, on a worker thread, and posts the result back to the host window.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::core::*;

use crate::shelf::{self, Kind};

/// Posted to the host window when a zip finishes: wParam is 1 on success.
pub const WM_ZIP_DONE: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 2;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

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

/// One Markdown link per file or link, as a list when there are several.
pub fn markdown(kinds: &[&Kind]) -> String {
    let links: Vec<String> = kinds
        .iter()
        .filter_map(|k| {
            let label = shelf::name(k).replace('[', "\\[").replace(']', "\\]");
            match k {
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
        let _ = std::fs::remove_file(&out);
        let ok = zip_command(&files, &out).status().is_ok_and(|s| s.success());
        unsafe {
            let _ = PostMessageW(HWND(target as *mut _), WM_ZIP_DONE, WPARAM(ok as usize), LPARAM(0));
        }
    });
}

fn zip_command(files: &[PathBuf], out: &Path) -> Command {
    let mut cmd = Command::new(r"C:\Windows\System32\tar.exe");
    // -a picks the format from the .zip extension. Each item is added from
    // its own folder, so the zip holds names, not full paths.
    cmd.arg("-a").arg("-c").arg("-f").arg(out);
    for f in files {
        if let (Some(dir), Some(name)) = (f.parent(), f.file_name()) {
            cmd.arg("-C").arg(dir).arg(name);
        }
    }
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
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
        let f = Kind::File("C:\\My Docs\\[draft] #1.md".into());
        assert_eq!(markdown(&[&f]), "[\\[draft\\] #1.md](file:///C:/My%20Docs/%5Bdraft%5D%20%231.md)");
        let (a, b) = (Kind::Link("www.e.com/x".into()), Kind::Link("https://e.com".into()));
        assert_eq!(markdown(&[&a, &b]), "- [e.com/x](https://www.e.com/x)\r\n- [e.com](https://e.com)");
    }

    #[test]
    fn zips_files_from_different_folders() {
        let dir = std::env::temp_dir().join("cairn-zip-test");
        let _ = std::fs::create_dir_all(dir.join("one"));
        let _ = std::fs::create_dir_all(dir.join("two"));
        let (a, b) = (dir.join("one").join("a.txt"), dir.join("two").join("b.txt"));
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        let out = dir.join("out.zip");
        let _ = std::fs::remove_file(&out);
        assert!(zip_command(&[a, b], &out).status().unwrap().success());
        let list = Command::new(r"C:\Windows\System32\tar.exe").arg("-tf").arg(&out).output().unwrap();
        let names = String::from_utf8_lossy(&list.stdout);
        assert!(names.contains("a.txt") && names.contains("b.txt"));
        // A real zip, not a tar.
        assert_eq!(&std::fs::read(&out).unwrap()[..2], b"PK");
    }
}
