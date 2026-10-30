//! phitpup-update.exe: the only part of PhitPup that connects to the
//! internet, and only when the person clicks "Check for updates" on the
//! About page. The installer puts it next to phitpup.exe.
//!
//! `phitpup-update check` fetches the website's release.json and says
//! whether a newer version is out. `phitpup-update install <version>`
//! downloads that version's installer from GitHub, checks its size and
//! SHA-256 against release.json, and starts it to update in place; the
//! installer closes PhitPup and starts the new version. With `--no-run`
//! after the version it stops once the checked installer is saved.
//!
//! It sends nothing but the two requests, with no cookies or identifiers,
//! and reports to the app with one line per step on standard output (see
//! `phitpup_core::update::Line`).

#![windows_subsystem = "windows"]

use std::io::Write;

use phitpup_core::update::{self, Line};

/// Refuse anything larger: the installer is about 2.5 MB.
const MAX_SIZE: u64 = 64 << 20;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let line = match args.first().map(String::as_str) {
        Some("check") => check(),
        Some("install") => match args.get(1) {
            Some(version) => install(version, args.get(2).is_some_and(|a| a == "--no-run")),
            None => Line::Failed("No version given.".into()),
        },
        _ => Line::Failed("Start this from the About page in PhitPup's settings.".into()),
    };
    say(&line);
}

fn say(line: &Line) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

fn latest() -> Result<update::Release, Line> {
    // The query string gets past the website's ten-minute cache.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let path = format!("{}?t={now}", update::RELEASE_JSON);
    let body = net::get(update::RELEASE_HOST, &path, 64 << 10, |_, _| {})?;
    let text = String::from_utf8_lossy(&body);
    update::parse(&text).ok_or_else(|| Line::Failed("The version information looks wrong.".into()))
}

fn check() -> Line {
    let release = match latest() {
        Ok(r) => r,
        Err(line) => return line,
    };
    if !update::newer(&release.version, env!("CARGO_PKG_VERSION")) {
        return Line::Current;
    }
    match release.setup {
        Some((_, size)) => Line::Available(release.version, size),
        None => Line::Failed(format!(
            "Version {} has no installer. Get it from the website.",
            release.version
        )),
    }
}

/// `no_run` stops after saving the checked installer, for testing.
fn install(version: &str, no_run: bool) -> Line {
    let release = match latest() {
        Ok(r) => r,
        Err(line) => return line,
    };
    let Some((sha, size)) = release.setup.filter(|_| release.version == version) else {
        return Line::Failed("A different version came out meanwhile. Check again.".into());
    };
    if size > MAX_SIZE {
        return Line::Failed("The installer is unexpectedly large.".into());
    }
    let mut shown = 0;
    let body = net::get(
        update::DOWNLOAD_HOST,
        &update::setup_path(version),
        MAX_SIZE,
        |done, total| {
            // A line every 64 KB is plenty for a progress bar.
            if done - shown >= 64 << 10 || done == total {
                shown = done;
                say(&Line::Progress(done, total.max(done)));
            }
        },
    );
    let body = match body {
        Ok(b) => b,
        Err(line) => return line,
    };
    if body.len() as u64 != size || net::sha256(&body).as_deref() != Some(sha.as_str()) {
        return Line::Failed("The download did not match the published SHA-256.".into());
    }
    let temp = std::env::temp_dir();
    // Installers left from earlier updates.
    if let Ok(entries) = std::fs::read_dir(&temp) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with("PhitPup-Setup-") && name.ends_with(".exe") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let file = temp.join(format!("PhitPup-Setup-{version}.exe"));
    if std::fs::write(&file, &body).is_err() {
        return Line::Failed("Could not save the installer.".into());
    }
    if no_run {
        return Line::Started;
    }
    // /UPDATE makes the installer keep the person's choices and start
    // PhitPup again when it is done (installer/phitpup.iss).
    match std::process::Command::new(&file)
        .args(["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/UPDATE"])
        .args(lang())
        .spawn()
    {
        Ok(_) => Line::Started,
        Err(_) => Line::Failed("Could not start the installer.".into()),
    }
}

/// `/LANG=` for Setup: the language PhitPup passes in `PHITPUP_LANG`.
fn lang() -> Option<String> {
    let lang = std::env::var("PHITPUP_LANG").ok()?;
    matches!(lang.as_str(), "th" | "en").then(|| format!("/LANG={lang}"))
}

#[cfg(windows)]
mod net {
    use phitpup_core::update::Line;
    use windows_sys::Win32::Networking::WinHttp::{
        INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE,
        WINHTTP_QUERY_CONTENT_LENGTH, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
        WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders,
        WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
    };
    use windows_sys::Win32::Security::Cryptography::{BCRYPT_SHA256_ALG_HANDLE, BCryptHash};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }

    /// Closes a WinHTTP handle when dropped.
    struct Handle(*mut core::ffi::c_void);

    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: a handle WinHTTP gave us, closed once.
                unsafe { WinHttpCloseHandle(self.0) };
            }
        }
    }

    fn offline() -> Line {
        Line::Failed("Could not connect. Check your internet connection.".into())
    }

    fn header(request: &Handle, which: u32) -> Option<u32> {
        let mut value = 0u32;
        let mut len = 4u32;
        // SAFETY: a 4-byte buffer for a numeric header.
        let ok = unsafe {
            WinHttpQueryHeaders(
                request.0,
                which | WINHTTP_QUERY_FLAG_NUMBER,
                std::ptr::null(),
                std::ptr::addr_of_mut!(value).cast(),
                &mut len,
                std::ptr::null_mut(),
            )
        };
        (ok != 0).then_some(value)
    }

    /// GET over HTTPS, following redirects, with the system's proxy
    /// settings. `progress` gets (bytes so far, expected total or 0).
    pub fn get(
        host: &str,
        path: &str,
        max: u64,
        mut progress: impl FnMut(u64, u64),
    ) -> Result<Vec<u8>, Line> {
        let agent = wide(concat!("PhitPup-Update/", env!("CARGO_PKG_VERSION")));
        // SAFETY: WinHTTP calls with NUL-terminated strings; every handle is
        // closed by `Handle`, children before parents (reverse drop order).
        unsafe {
            let session = Handle(WinHttpOpen(
                agent.as_ptr(),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                std::ptr::null(),
                std::ptr::null(),
                0,
            ));
            if session.0.is_null() {
                return Err(offline());
            }
            WinHttpSetTimeouts(session.0, 10_000, 10_000, 10_000, 20_000);
            let connection = Handle(WinHttpConnect(
                session.0,
                wide(host).as_ptr(),
                INTERNET_DEFAULT_HTTPS_PORT,
                0,
            ));
            if connection.0.is_null() {
                return Err(offline());
            }
            let request = Handle(WinHttpOpenRequest(
                connection.0,
                wide("GET").as_ptr(),
                wide(path).as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            ));
            if request.0.is_null()
                || WinHttpSendRequest(request.0, std::ptr::null(), 0, std::ptr::null(), 0, 0, 0)
                    == 0
                || WinHttpReceiveResponse(request.0, std::ptr::null_mut()) == 0
            {
                return Err(offline());
            }
            match header(&request, WINHTTP_QUERY_STATUS_CODE) {
                Some(200) => {}
                Some(404) => {
                    return Err(Line::Failed(
                        "The download is not there yet. Try again in a few minutes.".into(),
                    ));
                }
                Some(code) => {
                    return Err(Line::Failed(format!(
                        "The server answered {code}. Try again later."
                    )));
                }
                None => return Err(offline()),
            }
            let total = header(&request, WINHTTP_QUERY_CONTENT_LENGTH).map_or(0, u64::from);
            if total > max {
                return Err(Line::Failed("The download is unexpectedly large.".into()));
            }
            let mut body = Vec::with_capacity(total as usize);
            let mut buf = vec![0u8; 64 << 10];
            loop {
                let mut read = 0u32;
                if WinHttpReadData(
                    request.0,
                    buf.as_mut_ptr().cast(),
                    buf.len() as u32,
                    &mut read,
                ) == 0
                {
                    return Err(Line::Failed("The connection dropped. Try again.".into()));
                }
                if read == 0 {
                    break;
                }
                body.extend_from_slice(&buf[..read as usize]);
                if body.len() as u64 > max {
                    return Err(Line::Failed("The download is unexpectedly large.".into()));
                }
                progress(body.len() as u64, total);
            }
            Ok(body)
        }
    }

    /// Lowercase hex SHA-256, from Windows' own cryptography library.
    pub fn sha256(data: &[u8]) -> Option<String> {
        let mut out = [0u8; 32];
        // SAFETY: the SHA-256 pseudo-handle (Windows 10+), no key, a
        // 32-byte output buffer.
        let status = unsafe {
            BCryptHash(
                BCRYPT_SHA256_ALG_HANDLE,
                std::ptr::null(),
                0,
                data.as_ptr(),
                u32::try_from(data.len()).ok()?,
                out.as_mut_ptr(),
                32,
            )
        };
        (status == 0).then(|| out.iter().map(|b| format!("{b:02x}")).collect())
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn sha256_of_abc() {
            assert_eq!(
                super::sha256(b"abc").unwrap(),
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
            );
        }
    }
}

#[cfg(not(windows))]
mod net {
    use phitpup_core::update::Line;

    pub fn get(_: &str, _: &str, _: u64, _: impl FnMut(u64, u64)) -> Result<Vec<u8>, Line> {
        Err(Line::Failed("Updating works on Windows only.".into()))
    }

    pub fn sha256(_: &[u8]) -> Option<String> {
        None
    }
}
