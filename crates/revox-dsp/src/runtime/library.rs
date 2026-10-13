//! Locate ORT relative to this native module, independent of the host and cwd.

use std::{
    ffi::OsStr,
    io,
    path::{Path, PathBuf},
};

#[cfg(target_os = "macos")]
const LIBRARY_NAME: &str = "libonnxruntime.dylib";
#[cfg(target_os = "linux")]
const LIBRARY_NAME: &str = "libonnxruntime.so";
#[cfg(target_os = "windows")]
const LIBRARY_NAME: &str = "onnxruntime.dll";

pub(super) fn path() -> io::Result<PathBuf> {
    let module = module_path()?;
    let development_override = std::env::var_os("ORT_DYLIB_PATH");
    resolve(&module, development_override.as_deref())
}

fn resolve(module: &Path, development_override: Option<&OsStr>) -> io::Result<PathBuf> {
    let bundle = module.ancestors().find(|ancestor| {
        ancestor
            .extension()
            .and_then(OsStr::to_str)
            .is_some_and(|extension| {
                ["vst3", "component", "aaxplugin"]
                    .iter()
                    .any(|format| extension.eq_ignore_ascii_case(format))
            })
    });
    let directory = module
        .parent()
        .ok_or_else(|| io::Error::other("Revox native module has no parent directory"))?;
    let path = if let Some(bundle) = bundle {
        // An installed plug-in must use its own shipped runtime, even when a
        // host process has ORT_DYLIB_PATH set for an unrelated dependency.
        if cfg!(target_os = "macos") {
            bundle.join("Contents/Frameworks").join(LIBRARY_NAME)
        } else {
            directory.join(LIBRARY_NAME)
        }
    } else if let Some(path) = development_override {
        // Cargo tests and command-line DSP probes are outside native bundles.
        // Their explicit absolute path is supplied by runtime preparation.
        PathBuf::from(path)
    } else {
        directory.join(LIBRARY_NAME)
    };
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "ONNX Runtime library path must be absolute",
        ));
    }
    Ok(path)
}

// Its address identifies the loaded plug-in (or test/probe executable), never
// the DAW executable. This function is private so symbol interposition cannot
// redirect the address to another plug-in's copy.
#[inline(never)]
fn module_anchor() {}

#[cfg(windows)]
pub(super) struct PreparedLibrary(windows_sys::Win32::Foundation::HMODULE);

#[cfg(windows)]
impl Drop for PreparedLibrary {
    fn drop(&mut self) {
        // SAFETY: prepare acquired exactly this reference. ort::init_from has
        // acquired its own reference before this guard is dropped on success;
        // an initialization failure releases only our temporary reference.
        unsafe { windows_sys::Win32::Foundation::FreeLibrary(self.0) };
    }
}

#[cfg(windows)]
pub(super) fn prepare(path: &Path) -> io::Result<PreparedLibrary> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::LibraryLoader::{
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
    };

    let mut path: Vec<u16> = path.as_os_str().encode_wide().collect();
    if path.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "ONNX Runtime library path contains a NUL",
        ));
    }
    path.push(0);
    // SAFETY: path is absolute and NUL-terminated. The search flags resolve
    // app-local VC runtimes beside ORT, then Windows system libraries, without
    // changing the DAW's global DLL directories or searching its cwd/PATH.
    let module = unsafe {
        LoadLibraryExW(
            path.as_ptr(),
            std::ptr::null_mut(),
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    };
    if module.is_null() {
        return Err(io::Error::last_os_error());
    }
    Ok(PreparedLibrary(module))
}

#[cfg(unix)]
fn module_path() -> io::Result<PathBuf> {
    use std::{ffi::CStr, os::unix::ffi::OsStrExt};

    let mut info = std::mem::MaybeUninit::<libc::Dl_info>::uninit();
    // SAFETY: the anchor belongs to this loaded module for the duration of the
    // call, and dladdr writes a complete Dl_info on success.
    let found = unsafe { libc::dladdr(module_anchor as *const () as *const _, info.as_mut_ptr()) };
    if found == 0 {
        return Err(io::Error::other("could not locate the Revox native module"));
    }
    // SAFETY: dladdr reported success and initialized info.
    let info = unsafe { info.assume_init() };
    if info.dli_fname.is_null() {
        return Err(io::Error::other("Revox native module has no filename"));
    }
    // SAFETY: dladdr supplies a NUL-terminated name belonging to the loaded
    // module; copy it before returning and retain no loader-owned pointer.
    let name = unsafe { CStr::from_ptr(info.dli_fname) };
    std::fs::canonicalize(Path::new(OsStr::from_bytes(name.to_bytes())))
}

#[cfg(windows)]
fn module_path() -> io::Result<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::LibraryLoader::{
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        GetModuleFileNameW, GetModuleHandleExW,
    };

    let mut module = std::ptr::null_mut();
    // SAFETY: FROM_ADDRESS interprets the second argument as an address in our
    // loaded module, not a string. UNCHANGED_REFCOUNT gives a borrowed handle;
    // the caller keeps this module alive and we never free the handle.
    let found = unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            module_anchor as *const () as *const u16,
            &mut module,
        )
    };
    if found == 0 {
        return Err(io::Error::last_os_error());
    }
    // Windows extended paths are bounded by 32,767 UTF-16 code units.
    let mut path = vec![0_u16; 32_768];
    // SAFETY: the module handle remains valid and the writable buffer has the
    // reported capacity; the returned length excludes the terminating NUL.
    let length =
        unsafe { GetModuleFileNameW(module, path.as_mut_ptr(), path.len() as u32) } as usize;
    if length == 0 {
        return Err(io::Error::last_os_error());
    }
    if length >= path.len() {
        return Err(io::Error::other("Revox native module path was truncated"));
    }
    Ok(std::ffi::OsString::from_wide(&path[..length]).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_runtime_is_local_even_with_host_override() {
        let root = std::env::temp_dir();
        let host_override = root.join("unrelated-host-runtime");
        // Windows and commonly used macOS filesystems allow case changes to
        // installed bundle names; those must not enable the host override.
        for extension in ["vst3", "VST3"] {
            let bundle = root.join(format!("Heron Revox.{extension}"));
            let module = bundle.join("Contents").join("native").join("Heron Revox");
            let resolved = resolve(&module, Some(host_override.as_os_str())).unwrap();
            let expected = if cfg!(target_os = "macos") {
                bundle.join("Contents/Frameworks").join(LIBRARY_NAME)
            } else {
                module.parent().unwrap().join(LIBRARY_NAME)
            };
            assert_eq!(resolved, expected);
        }
    }

    #[test]
    fn standalone_probe_accepts_only_absolute_runtime_override() {
        let root = std::env::temp_dir();
        let module = root.join("probe");
        let runtime = root.join(LIBRARY_NAME);
        assert_eq!(
            resolve(&module, Some(runtime.as_os_str())).unwrap(),
            runtime
        );
        assert_eq!(
            resolve(&module, Some(OsStr::new("relative-runtime")))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn module_discovery_identifies_current_test_executable() {
        let actual = std::fs::canonicalize(module_path().unwrap()).unwrap();
        let expected = std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
        assert_eq!(actual, expected);
    }
}
