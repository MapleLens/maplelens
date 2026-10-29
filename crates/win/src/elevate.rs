//! Running as administrator, and why.
//!
//! MapleStory is started by its launcher as administrator. While a window of a program running that way is in front,
//! Windows' User Interface Privilege Isolation hides that program's input from programs running without it: the
//! pointer's position still reads (`GetCursorPos`), but the mouse buttons read as never pressed
//! (`GetAsyncKeyState`). MapleLens's cards are click-through (rule 3) and nothing hooks the mouse (rule 1), so reading
//! the button is the only way it sees a press on a card; without it, a monster cannot be clicked and a card cannot
//! be dragged. Hover and the price cards keep working either way.
//!
//! So MapleLens asks once, at start, to run as administrator (the Windows prompt), by starting itself again with the
//! "runas" verb and leaving. Declined, it carries on without, minus clicks and drags. Started at sign-in from the Run
//! key, it asks then. Being elevated raises what a compromised update could do, which is why the update chain
//! (`update`) must stay as strict as it is.

use std::path::Path;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::LibraryLoader::{LOAD_LIBRARY_SEARCH_SYSTEM32, SetDefaultDllDirectories};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HSTRING, w};

/// Is this process running as administrator?
pub fn is_elevated() -> bool {
    let mut token = HANDLE::default();
    let mut elevation = TOKEN_ELEVATION::default();
    let mut size = 0;
    // SAFETY: our own process's token, queried into a local of the right size, and closed after.
    unsafe {
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let read = GetTokenInformation(
            token,
            TokenElevation,
            Some((&raw mut elevation).cast()),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut size,
        );
        let _ = CloseHandle(token);
        read.is_ok() && elevation.TokenIsElevated != 0
    }
}

/// Libraries load from System32 only, never from the exe's folder or the working directory: players run the exe from
/// wherever they unzipped it (Downloads, often), any program can drop a DLL beside it there, and this process runs as
/// administrator. The exe's own imports are covered at link time (`/DEPENDENTLOADFLAG` in build.rs, and the static C
/// runtime in .cargo/config.toml); this covers what Windows' own libraries load later.
pub fn restrict_dll_search() {
    // SAFETY: a process-wide setting, given a valid flag; it cannot fail on the Windows versions MapleLens runs on.
    let _ = unsafe { SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32) };
}

/// Start this exe again as administrator with the same arguments; true when it started (this copy should then leave).
/// False when already elevated, or when the prompt was declined or failed.
pub fn relaunch_elevated() -> bool {
    if is_elevated() {
        return false;
    }
    let Ok(exe) = std::env::current_exe() else { return false };
    let arguments: Vec<String> = std::env::args().skip(1).map(|a| quote(&a)).collect();
    let folder = std::env::current_dir().unwrap_or_else(|_| exe.parent().unwrap_or(Path::new(".")).to_path_buf());
    let (exe, arguments, folder) =
        (HSTRING::from(exe.as_os_str()), HSTRING::from(arguments.join(" ")), HSTRING::from(folder.as_os_str()));
    // SAFETY: plain strings that outlive the call; ShellExecuteW shows the prompt and returns.
    let started = unsafe { ShellExecuteW(None, w!("runas"), &exe, &arguments, &folder, SW_SHOWNORMAL) };
    // Above 32 is success; anything else (the prompt declined is 5, access denied) carries on here.
    started.0 as isize > 32
}

/// An argument as the C runtime splits command lines: quoted when it has a space or a quote. Backslashes are plain
/// letters there except before a quote, where they escape in pairs, so those are doubled: `C:\My Dir\` quoted as is
/// would end in `\"`, an escaped quote, and run on into the next argument.
fn quote(argument: &str) -> String {
    if !argument.is_empty() && !argument.contains([' ', '\t', '"']) {
        return argument.to_string();
    }
    let mut quoted = String::from('"');
    let mut backslashes = 0;
    for c in argument.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                quoted.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            c => {
                quoted.extend(std::iter::repeat_n('\\', backslashes));
                quoted.push(c);
                backslashes = 0;
            }
        }
    }
    quoted.extend(std::iter::repeat_n('\\', backslashes * 2));
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::UI::Shell::CommandLineToArgvW;
    use windows::core::HSTRING;

    use super::quote;

    #[test]
    fn arguments_keep_their_spaces() {
        assert_eq!(quote("--debug"), "--debug");
        assert_eq!(quote(r"C:\My Games\list.json"), r#""C:\My Games\list.json""#);
        assert_eq!(quote(""), r#""""#);
        assert_eq!(quote(r"C:\My Dir\"), r#""C:\My Dir\\""#);
        assert_eq!(quote(r#"say \"hi\""#), r#""say \\\"hi\\\"""#);
    }

    /// The arguments after the program's name, as Windows splits `line`.
    fn split(line: &str) -> Vec<String> {
        let line = HSTRING::from(line);
        let mut count = 0;
        // SAFETY: a NUL-terminated string that outlives the call; the array Windows returns is read within its count
        // and freed once, after its strings are copied.
        unsafe {
            let argv = CommandLineToArgvW(&line, &mut count);
            assert!(!argv.is_null());
            let all = std::slice::from_raw_parts(argv, count as usize);
            let arguments = all.iter().skip(1).map(|a| a.to_string().unwrap()).collect();
            let _ = LocalFree(Some(HLOCAL(argv.cast())));
            arguments
        }
    }

    #[test]
    fn every_argument_reaches_the_elevated_copy_as_it_was() {
        let arguments = [
            "--snapshot",
            r"C:\My Dir\",
            r"C:\Dir\",
            "",
            r#"say "hi""#,
            r#"a\"b"#,
            r"\\server\share dir\\",
            "tab\there",
            r#"ends in a quote""#,
            r"\",
        ];
        let line: Vec<String> = arguments.iter().map(|a| quote(a)).collect();
        assert_eq!(split(&format!("maplelens.exe {}", line.join(" "))), arguments);
    }
}
