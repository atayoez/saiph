//! Saiph's runner. `cargo kernel`, `cargo board` and `cargo screenshot` are
//! aliases for `cargo run -p kernel` (see .cargo/config.toml): Cargo builds
//! the kernel, then its runner hook starts this program with the kernel file
//! and the rest of the command line:
//!
//!   runner <kernel> [tcg|kvm] [words...]    boot it in QEMU; the words become
//!                                           the kernel's command line
//!   runner <kernel> board [remove]          boot it once on this machine (the
//!                                           Orion O6), or remove it again
//!   runner <kernel> screenshot [file.png]   save what QEMU's screen shows

use hadris_fat::format::{FatFormatOptions, FatTypeSelection, FatVolumeFormatter};
use hadris_fat::{FatDir, FatVolume, FatVolumeWriteExt};
use std::env;
use std::error::Error;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

const USAGE: &str = "usage:
  cargo kernel [tcg|kvm] [words...]   build Saiph and boot it in QEMU
  cargo board [remove]                boot Saiph once on this machine, or remove it
  cargo screenshot [file.png]         save what QEMU's screen shows";

/// QEMU's UEFI firmware, from the openSUSE package qemu-uefi-aarch64.
const AAVMF_CODE: &str = "/usr/share/qemu/aavmf-aarch64-code.bin";
const AAVMF_VARS: &str = "/usr/share/qemu/aavmf-aarch64-vars.bin";
/// The EFI system partition, and Saiph's systemd-boot menu entry on it.
const ESP: &str = "/boot/efi";
const ENTRY: &str = "saiph.conf";
/// Where the hardware probe installed itself (removed from the repository;
/// see the git tag probe-final). `cargo board remove` cleans it up too.
const OLD_PROBE_ENTRY: &str = "orion-probe.conf";
/// systemd-boot's one-time boot entry, an EFI variable.
const ONESHOT_VAR: &str = "/sys/firmware/efi/efivars/LoaderEntryOneShot-4a67b082-0a4c-41cf-b6c7-440b29bb8c4f";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        // The steps that write to the ESP, run as root through pkexec.
        Some("esp-install") => esp_install(&args[1..]),
        Some("esp-remove") => esp_remove(&args[1..]),
        Some(kernel) => {
            let mut rest = &args[1..];
            // `cargo kernel -- kvm` passes its `--` through.
            if rest.first().is_some_and(|a| a == "--") {
                rest = &rest[1..];
            }
            run(Path::new(kernel), rest)
        }
        None => Err(USAGE.into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run(kernel: &Path, args: &[String]) -> Result {
    let kernel = fs::canonicalize(kernel)?;
    match args.first().map(String::as_str) {
        Some("board") => board(&kernel, &args[1..]),
        Some("screenshot") => screenshot(&args[1..]),
        Some(mode @ ("tcg" | "kvm")) => qemu(&kernel, mode, &args[1..]),
        _ => qemu(&kernel, "tcg", args),
    }
}

/// The workspace root (this crate's parent folder).
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

/// Limine, bundled in the repository (see limine/README.md).
fn limine_efi() -> PathBuf {
    root().join("limine/BOOTAA64.EFI")
}

/// Where QEMU's boot image and control socket go.
fn qemu_dir() -> PathBuf {
    root().join("target/kernel-qemu")
}

fn need(path: &str, package: &str) -> Result {
    if Path::new(path).exists() {
        Ok(())
    } else {
        Err(format!("missing {path}: install it with `sudo zypper install {package}`").into())
    }
}

/// limine/limine.conf, plus a `cmdline:` line when there are words for it.
fn limine_conf(base: &str, cmdline: &str) -> String {
    let mut conf = base.to_string();
    if !cmdline.is_empty() {
        conf.push_str(&format!("    cmdline: {cmdline}\n"));
    }
    conf
}

fn read_limine_conf(cmdline: &str) -> Result<String> {
    Ok(limine_conf(&fs::read_to_string(root().join("limine/limine.conf"))?, cmdline))
}

/// Boot the kernel in QEMU. `tcg` emulates a CPU with EL2, so the kernel is
/// entered at EL2 as on the board; `kvm` runs it on this machine's own cores,
/// entered at EL1. The serial console is this terminal (quit with Ctrl+A,
/// then X). The screen (ramfb) is not shown; `cargo screenshot` saves it.
fn qemu(kernel: &Path, mode: &str, words: &[String]) -> Result {
    need(AAVMF_CODE, "qemu-uefi-aarch64")?;
    let dir = qemu_dir();
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir)?;
    let image = dir.join("esp.img");
    make_image(&image, kernel, &read_limine_conf(&words.join(" "))?)?;

    let machine: &[&str] = match mode {
        "kvm" => &["-M", "virt,gic-version=3", "-cpu", "host", "-accel", "kvm"],
        _ => &["-M", "virt,gic-version=3,virtualization=on", "-cpu", "max"],
    };
    let error = Command::new("qemu-system-aarch64")
        .args(machine)
        .args(["-m", "1G"])
        .arg("-drive")
        .arg(format!("if=pflash,format=raw,readonly=on,file={AAVMF_CODE}"))
        .arg("-drive")
        .arg(format!("if=pflash,format=raw,snapshot=on,file={AAVMF_VARS}"))
        .arg("-drive")
        .arg(format!("if=virtio,format=raw,file={}", image.display()))
        .args(["-device", "ramfb", "-qmp"])
        .arg(format!("unix:{},server=on,wait=off", dir.join("qmp.sock").display()))
        .args(["-nographic", "-no-reboot"])
        .exec();
    match error.kind() {
        io::ErrorKind::NotFound => Err("qemu-system-aarch64 not found: install it with `sudo zypper install qemu-arm`".into()),
        _ => Err(format!("qemu-system-aarch64: {error}").into()),
    }
}

/// A FAT disk image like a USB stick: Limine as the removable-media boot
/// program (\EFI\BOOT\BOOTAA64.EFI), its config next to it, and the kernel
/// in \EFI\saiph.
fn make_image(path: &Path, kernel: &Path, limine_conf: &str) -> Result {
    const SIZE: u64 = 64 << 20;
    let file = File::options().read(true).write(true).create(true).truncate(true).open(path)?;
    file.set_len(SIZE)?;
    let options = FatFormatOptions::new(SIZE).fat_type(FatTypeSelection::Fat32).volume_label("SAIPH");
    let volume = FatVolumeFormatter::format(file, options)?;
    let root = volume.root_dir();
    let efi = volume.create_dir(&root, "EFI")?;
    let boot = volume.create_dir(&efi, "BOOT")?;
    add_file(&volume, &boot, "BOOTAA64.EFI", &fs::read(limine_efi())?)?;
    add_file(&volume, &boot, "limine.conf", limine_conf.as_bytes())?;
    let saiph = volume.create_dir(&efi, "saiph")?;
    add_file(&volume, &saiph, "kernel", &fs::read(kernel)?)?;
    Ok(())
}

fn add_file(volume: &FatVolume<File>, dir: &FatDir<'_, File>, name: &str, mut data: &[u8]) -> Result {
    let entry = volume.create_file(dir, name)?;
    let mut writer = volume.write_file(&entry)?;
    while !data.is_empty() {
        let written = writer.write(data)?;
        data = &data[written..];
    }
    writer.finish()?;
    Ok(())
}

/// Save what the running QEMU's screen shows, through its control socket
/// (QMP: one JSON command per line, one JSON answer per command).
fn screenshot(args: &[String]) -> Result {
    let out = match args.first() {
        Some(path) => env::current_dir()?.join(path),
        None => qemu_dir().join("screen.png"),
    };
    let socket = qemu_dir().join("qmp.sock");
    let stream = UnixStream::connect(&socket).map_err(|e| format!("no QEMU from `cargo kernel` is running ({e})"))?;
    let mut replies = BufReader::new(stream.try_clone()?);
    let mut greeting = String::new();
    replies.read_line(&mut greeting)?;
    qmp(&stream, &mut replies, r#"{"execute":"qmp_capabilities"}"#)?;
    let _ = fs::remove_file(&out);
    let filename = json_string(&out.to_string_lossy());
    qmp(&stream, &mut replies, &format!(r#"{{"execute":"screendump","arguments":{{"filename":{filename},"format":"png"}}}}"#))?;
    println!("{}", out.display());
    Ok(())
}

/// Send one QMP command and wait for its answer, skipping events.
fn qmp(mut stream: &UnixStream, replies: &mut BufReader<UnixStream>, command: &str) -> Result {
    writeln!(stream, "{command}")?;
    let mut line = String::new();
    loop {
        line.clear();
        if replies.read_line(&mut line)? == 0 {
            return Err("QEMU closed its control socket".into());
        }
        if line.contains("\"return\"") {
            return Ok(());
        }
        if line.contains("\"error\"") {
            return Err(format!("QEMU: {}", line.trim()).into());
        }
    }
}

fn json_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `cargo board`: copy Limine, its config and the kernel to the ESP and make
/// Saiph the next boot's one-time entry. On the board the kernel restarts the
/// machine 60 s after it finishes; the one-time entry is used up by then, so
/// Linux comes back. `cargo board remove` deletes it all again.
fn board(kernel: &Path, args: &[String]) -> Result {
    match args.first().map(String::as_str) {
        None => {
            let conf = root().join("target/board-limine.conf");
            fs::write(&conf, read_limine_conf("reboot-after=60")?)?;
            as_root(&["esp-install".as_ref(), ESP.as_ref(), kernel.as_os_str(), conf.as_os_str()])?;
            if oneshot().as_deref() == Some(ENTRY) {
                println!("Armed: the next boot runs Saiph once, then shows the systemd-boot menu.");
            } else {
                eprintln!("Warning: the one-time boot entry is not {ENTRY}; the next boot may skip Saiph.");
            }
            print!("Reboot now? [y/N] ");
            io::stdout().flush()?;
            let mut answer = String::new();
            io::stdin().read_line(&mut answer)?;
            if matches!(answer.trim(), "y" | "Y" | "yes") {
                Command::new("systemctl").arg("reboot").status()?;
            }
            Ok(())
        }
        Some("remove") => {
            as_root(&["esp-remove".as_ref(), ESP.as_ref()])?;
            println!("Removed Saiph (and the old hardware probe, if it was there) from the ESP.");
            Ok(())
        }
        Some(other) => Err(format!("unknown board command `{other}`\n{USAGE}").into()),
    }
}

/// Run this program again as root through pkexec (one password dialog) for
/// the steps that write to the ESP, which only root may change.
fn as_root(args: &[&OsStr]) -> Result {
    let status = Command::new("pkexec").arg(env::current_exe()?).args(args).status()?;
    if status.success() {
        Ok(())
    } else {
        Err("the step as root failed, or the password dialog was cancelled".into())
    }
}

/// As root: `esp-install <esp> <kernel> <limine.conf>`.
fn esp_install(args: &[String]) -> Result {
    let [esp, kernel, conf] = args else {
        return Err("esp-install: expected <esp> <kernel> <limine.conf>".into());
    };
    let esp = Path::new(esp);
    let dir = esp.join("EFI/saiph");
    fs::create_dir_all(&dir)?;
    fs::copy(limine_efi(), dir.join("limine.efi"))?;
    fs::copy(conf, dir.join("limine.conf"))?;
    fs::copy(kernel, dir.join("kernel"))?;
    fs::write(esp.join("loader/entries").join(ENTRY), "title   Saiph\nefi     /EFI/saiph/limine.efi\n")?;
    run_checked(Command::new("bootctl").args(["set-oneshot", ENTRY]))
}

/// As root: `esp-remove <esp>`. Also points a one-time boot that still names
/// a removed entry back at the default one.
fn esp_remove(args: &[String]) -> Result {
    let [esp] = args else {
        return Err("esp-remove: expected <esp>".into());
    };
    let esp = Path::new(esp);
    for dir in ["EFI/saiph", "EFI/orion-probe"] {
        if esp.join(dir).exists() {
            fs::remove_dir_all(esp.join(dir))?;
        }
    }
    for entry in [ENTRY, OLD_PROBE_ENTRY] {
        let path = esp.join("loader/entries").join(entry);
        if path.exists() {
            fs::remove_file(path)?;
        }
    }
    if matches!(oneshot().as_deref(), Some(ENTRY | OLD_PROBE_ENTRY)) {
        run_checked(Command::new("bootctl").args(["set-oneshot", "@default"]))?;
    }
    Ok(())
}

fn run_checked(command: &mut Command) -> Result {
    let status = command.status()?;
    if status.success() { Ok(()) } else { Err(format!("{command:?} failed: {status}").into()) }
}

/// systemd-boot's one-time boot entry, if one is set.
fn oneshot() -> Option<String> {
    decode_efi_string(&fs::read(ONESHOT_VAR).ok()?)
}

/// An EFI string variable: 4 bytes of attributes, then UTF-16LE text that
/// ends in a NUL.
fn decode_efi_string(raw: &[u8]) -> Option<String> {
    let units: Vec<u16> = raw.get(4..)?.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    Some(String::from_utf16_lossy(&units).trim_end_matches('\0').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn efi_strings_decode() {
        let mut raw = vec![7, 0, 0, 0];
        for unit in "saiph.conf\0".encode_utf16() {
            raw.extend(unit.to_le_bytes());
        }
        assert_eq!(decode_efi_string(&raw).as_deref(), Some("saiph.conf"));
        assert_eq!(decode_efi_string(&[1, 2]), None);
    }

    #[test]
    fn limine_conf_gets_a_cmdline_only_when_there_are_words() {
        assert_eq!(limine_conf("/Saiph\n", ""), "/Saiph\n");
        assert_eq!(limine_conf("/Saiph\n", "wx-test"), "/Saiph\n    cmdline: wx-test\n");
    }

    #[test]
    fn json_strings_are_escaped() {
        assert_eq!(json_string(r#"a"b\c"#), r#""a\"b\\c""#);
        assert_eq!(json_string("x\ny"), r#""x\u000ay""#);
    }

    #[test]
    fn boot_image_holds_limine_its_config_and_the_kernel() {
        use hadris_fat::FatVolumeReadExt;
        let dir = env::temp_dir().join(format!("saiph-image-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let kernel = dir.join("kernel");
        fs::write(&kernel, vec![0x5a; 300_000]).unwrap();
        let image = dir.join("esp.img");
        make_image(&image, &kernel, "/Saiph\n").unwrap();

        let volume = FatVolume::open(File::open(&image).unwrap()).unwrap();
        let efi = volume.root_dir().open_dir("EFI").unwrap();
        let saiph = efi.open_dir("saiph").unwrap();
        let entry = saiph.find("kernel").unwrap().expect("kernel missing");
        let mut reader = volume.read_file(&entry).unwrap();
        let mut back = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            let n = reader.read(&mut buffer).unwrap();
            if n == 0 {
                break;
            }
            back.extend_from_slice(&buffer[..n]);
        }
        assert_eq!(back, vec![0x5a; 300_000]);
        assert!(efi.open_dir("BOOT").unwrap().find("limine.conf").unwrap().is_some());
        fs::remove_dir_all(&dir).unwrap();
    }
}
