//! Everything the probe prints goes to the console and into
//! \EFI\orion-probe\report.txt on the volume the probe was loaded from.
//!
//! After every section `flush` reopens the file, appends the new text and
//! closes it again, so a hang later in the run still leaves the earlier
//! sections on disk.

use alloc::string::String;
use uefi::boot;
use uefi::proto::media::file::{Directory, File, FileAttribute, FileMode, RegularFile};
use uefi::{CString16, Status, println};

/// Print a line and keep it for the report: `say!(report, "x = {x}")`.
macro_rules! say {
    ($report:expr, $($arg:tt)*) => {
        $report.line(&alloc::format!($($arg)*))
    };
}
pub(crate) use say;

const REPORT: &str = "\\EFI\\orion-probe\\report.txt";

pub struct Report {
    /// Text not yet written to the file.
    pending: String,
    dir: Option<Directory>,
    /// Why the file could not be opened, if it could not.
    pub file_error: Option<String>,
}

fn path16(path: &str) -> CString16 {
    CString16::try_from(path).unwrap_or_default()
}

/// Open `path` for writing, deleting an older file of that name first.
fn create(root: &mut Directory, path: &str) -> uefi::Result<RegularFile> {
    let name = path16(path);
    if let Ok(old) = root.open(&name, FileMode::ReadWrite, FileAttribute::empty()) {
        old.delete()?;
    }
    root.open(&name, FileMode::CreateReadWrite, FileAttribute::empty())?
        .into_regular_file()
        .ok_or_else(|| Status::ACCESS_DENIED.into())
}

impl Report {
    pub fn new() -> Self {
        let mut r = Report { pending: String::new(), dir: None, file_error: None };
        match Self::open() {
            Ok(dir) => r.dir = Some(dir),
            Err(e) => r.file_error = Some(e),
        }
        r
    }

    /// Open the boot volume, create the folders and start an empty report.txt.
    fn open() -> Result<Directory, String> {
        let mut volume = boot::get_image_file_system(boot::image_handle())
            .map_err(|e| alloc::format!("no file system on the boot volume ({:?})", e.status()))?;
        let mut root = volume.open_volume().map_err(|e| alloc::format!("cannot open the volume ({:?})", e.status()))?;
        for dir in ["\\EFI", "\\EFI\\orion-probe", "\\EFI\\orion-probe\\acpi"] {
            root.open(&path16(dir), FileMode::CreateReadWrite, FileAttribute::DIRECTORY)
                .map_err(|e| alloc::format!("cannot create {dir} ({:?})", e.status()))?;
        }
        create(&mut root, REPORT)
            .map_err(|e| alloc::format!("cannot create report.txt ({:?})", e.status()))?
            .close();
        Ok(root)
    }

    pub fn line(&mut self, s: &str) {
        println!("{s}");
        self.pending.push_str(s);
        self.pending.push('\n');
    }

    pub fn section(&mut self, title: &str) {
        self.line("");
        self.line(&alloc::format!("== {title} =="));
    }

    /// Append the pending text to report.txt and make the firmware write it out.
    pub fn flush(&mut self) {
        let Some(dir) = self.dir.as_mut() else { return };
        let result = (|| {
            let mut f = dir
                .open(&path16(REPORT), FileMode::ReadWrite, FileAttribute::empty())
                .map_err(|e| alloc::format!("reopen failed ({:?})", e.status()))?
                .into_regular_file()
                .ok_or_else(|| String::from("not a regular file"))?;
            // Position u64::MAX means "end of file" in UEFI.
            f.set_position(RegularFile::END_OF_FILE).map_err(|e| alloc::format!("seek failed ({:?})", e.status()))?;
            f.write(self.pending.as_bytes()).map_err(|e| alloc::format!("write failed ({:?})", e.status()))?;
            f.close();
            Ok::<(), String>(())
        })();
        match result {
            Ok(()) => self.pending.clear(),
            Err(e) => println!("(report.txt: {e})"),
        }
    }

    /// Write an extra file below \EFI\orion-probe (for example an ACPI table).
    pub fn save_file(&mut self, name: &str, data: &[u8]) -> Result<(), String> {
        let dir = self.dir.as_mut().ok_or_else(|| String::from("no writable boot volume"))?;
        let mut f = create(dir, &alloc::format!("\\EFI\\orion-probe\\{name}")).map_err(|e| alloc::format!("{:?}", e.status()))?;
        f.write(data).map_err(|e| alloc::format!("{:?}", e.status()))?;
        f.flush().map_err(|e| alloc::format!("{:?}", e.status()))?;
        Ok(())
    }
}
