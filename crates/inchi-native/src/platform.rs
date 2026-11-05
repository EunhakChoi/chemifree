//! Linux loader and exact classic API ABI from the official v1.07.5 public header:
//! https://github.com/IUPAC-InChI/InChI/blob/v1.07.5/INCHI-1-SRC/INCHI_BASE/src/inchi_api.h
//! MAXVAL=20, ATOM_EL_LEN=6, AT_NUM=signed short, S_CHAR=signed char.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::fs::File;
use std::io::Read;
use std::mem;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::ptr;
use std::sync::Mutex;

use sha2::{Digest, Sha256};

use crate::{
    BondOrder, Error, ErrorKind, Hydrogens, Molecule, Output, Parity, Radical, Registration, Stereo,
};

static NATIVE_CALL: Mutex<()> = Mutex::new(());
const MAX_LIBRARY_BYTES: u64 = 128 * 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 1024 * 1024;

#[repr(C)]
struct InchiAtom {
    x: f64,
    y: f64,
    z: f64,
    neighbor: [i16; 20],
    bond_type: [i8; 20],
    bond_stereo: [i8; 20],
    elname: [c_char; 6],
    num_bonds: i16,
    num_iso_h: [i8; 4],
    isotopic_mass: i16,
    radical: i8,
    charge: i8,
}

#[repr(C)]
struct InchiStereo {
    neighbor: [i16; 4],
    central_atom: i16,
    kind: i8,
    parity: i8,
}

#[repr(C)]
struct InchiInput {
    atom: *mut InchiAtom,
    stereo: *mut InchiStereo,
    options: *mut c_char,
    num_atoms: i16,
    num_stereo: i16,
}

#[repr(C)]
struct InchiOutput {
    inchi: *mut c_char,
    aux: *mut c_char,
    message: *mut c_char,
    log: *mut c_char,
}

type Generate = unsafe extern "C" fn(*mut InchiInput, *mut InchiOutput) -> c_int;
type Free = unsafe extern "C" fn(*mut InchiOutput);
type Key = unsafe extern "C" fn(*const c_char, *mut c_char) -> c_int;

#[link(name = "dl")]
unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
    fn dlerror() -> *mut c_char;
}

pub(super) struct Library {
    // Keep the descriptor open: reusing /proc/self/fd/N for a different library while
    // the old handle is alive could hit the dynamic loader's pathname cache.
    registered_file: Option<File>,
    handle: *mut c_void,
    generate: Generate,
    free: Free,
    key: Key,
}

// SAFETY: the handle is owned by Library, function pointers remain valid until Drop, and
// every load/generate/free/close operation is protected by the process-wide NATIVE_CALL lock.
unsafe impl Send for Library {}
// SAFETY: no native mutable state is accessed outside the same process-wide lock.
unsafe impl Sync for Library {}

impl Library {
    pub(super) fn load(registration: &Registration) -> Result<Self, Error> {
        // A registration may point to a FIFO or device. Open without waiting for a
        // writer, then inspect this descriptor before hashing anything. rustix supplies
        // the architecture-correct Linux O_NONBLOCK value; regular-file reads are unchanged.
        let mut file = File::options()
            .read(true)
            .custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32)
            .open(&registration.library_path)
            .map_err(|cause| {
                crate::error(
                    ErrorKind::Unavailable,
                    format!("Cannot open registered InChI library: {cause}"),
                )
            })?;
        if !file
            .metadata()
            .map_err(|cause| crate::error(ErrorKind::Unavailable, cause.to_string()))?
            .is_file()
        {
            return Err(crate::error(
                ErrorKind::Registration,
                "The registered InChI path must be a regular file.",
            ));
        }
        if file
            .metadata()
            .map_err(|cause| crate::error(ErrorKind::Unavailable, cause.to_string()))?
            .len()
            > MAX_LIBRARY_BYTES
        {
            return Err(crate::error(
                ErrorKind::Limit,
                "The registered native library exceeds 128 MiB.",
            ));
        }
        let mut digest = Sha256::new();
        let mut total = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let count = file.read(&mut buffer).map_err(|cause| {
                crate::error(
                    ErrorKind::Unavailable,
                    format!("Cannot hash registered InChI library: {cause}"),
                )
            })?;
            if count == 0 {
                break;
            }
            total += count as u64;
            if total > MAX_LIBRARY_BYTES {
                return Err(crate::error(
                    ErrorKind::Limit,
                    "The registered native library exceeds 128 MiB.",
                ));
            }
            digest.update(&buffer[..count]);
        }
        let actual = format!("{:x}", digest.finalize());
        if actual != registration.sha256 {
            return Err(crate::error(
                ErrorKind::Registration,
                "The registered InChI library SHA-256 does not match.",
            ));
        }
        // Load the same open inode that was hashed, rather than resolving a path a second
        // time. The registration still assumes trusted bytes not concurrently modified in place.
        let path = CString::new(format!("/proc/self/fd/{}", file.as_raw_fd())).map_err(|_| {
            crate::error(ErrorKind::Registration, "Invalid library descriptor path.")
        })?;
        let _guard = NATIVE_CALL.lock().map_err(|_| {
            crate::error(
                ErrorKind::Unavailable,
                "The native InChI serialization lock is poisoned.",
            )
        })?;
        // SAFETY: path is NUL-terminated and lives through the call. This explicit trusted
        // registration authorizes executing library constructors. RTLD_NOW=2 resolves symbols now.
        let handle = unsafe { dlopen(path.as_ptr(), 2) };
        if handle.is_null() {
            return Err(crate::error(ErrorKind::Unavailable, loader_error()));
        }
        let functions = (|| {
            let generate = symbol(handle, c"GetStdINCHI")?;
            let free = symbol(handle, c"FreeStdINCHI")?;
            let key = symbol(handle, c"GetStdINCHIKeyFromStdINCHI")?;
            // SAFETY: symbol names and exact C signatures are from the pinned public header;
            // registration explicitly trusts a matching official build. POSIX dlsym returns
            // callable function addresses and these pointers remain tied to this library handle.
            Ok(unsafe {
                (
                    mem::transmute::<*mut c_void, Generate>(generate),
                    mem::transmute::<*mut c_void, Free>(free),
                    mem::transmute::<*mut c_void, Key>(key),
                )
            })
        })();
        match functions {
            Ok((generate, free, key)) => Ok(Self {
                registered_file: Some(file),
                handle,
                generate,
                free,
                key,
            }),
            Err(cause) => {
                // SAFETY: handle came from successful dlopen, has no outstanding calls and
                // was not transferred into a Library. The native lock is still held.
                unsafe {
                    dlclose(handle);
                }
                Err(cause)
            }
        }
    }

    pub(super) fn generate(&self, molecule: &Molecule, timeout: u16) -> Result<Output, Error> {
        // Caller validated counts, element names, adjacency capacities and every stereo index.
        let mut atoms: Vec<InchiAtom> = molecule
            .atoms
            .iter()
            .map(|atom| {
                let mut element = [0; 6];
                for (target, source) in element.iter_mut().zip(atom.element.bytes()) {
                    *target = source as c_char;
                }
                InchiAtom {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    neighbor: [0; 20],
                    bond_type: [0; 20],
                    bond_stereo: [0; 20],
                    elname: element,
                    num_bonds: 0,
                    num_iso_h: [
                        match atom.hydrogens {
                            Hydrogens::Automatic => -1,
                            Hydrogens::Count(count) => count as i8,
                        },
                        0,
                        0,
                        0,
                    ],
                    isotopic_mass: atom.isotope.unwrap_or(0) as i16,
                    radical: match atom.radical {
                        Radical::None => 0,
                        Radical::Singlet => 1,
                        Radical::Doublet => 2,
                        Radical::Triplet => 3,
                    },
                    charge: atom.charge,
                }
            })
            .collect();
        for bond in &molecule.bonds {
            let kind = match bond.order {
                BondOrder::Single => 1,
                BondOrder::Double => 2,
                BondOrder::Triple => 3,
            };
            for (first, second) in [(bond.begin, bond.end), (bond.end, bond.begin)] {
                let atom = &mut atoms[first];
                let index = atom.num_bonds as usize;
                atom.neighbor[index] = second as i16;
                atom.bond_type[index] = kind;
                atom.num_bonds += 1;
            }
        }
        let mut stereo: Vec<InchiStereo> = molecule
            .stereo
            .iter()
            .map(|stereo| {
                let (neighbors, center, kind, parity) = match stereo {
                    Stereo::Tetrahedral {
                        center,
                        neighbors,
                        parity,
                    } => (neighbors, *center as i16, 2, parity),
                    Stereo::DoubleBond { neighbors, parity } => (neighbors, -1, 1, parity),
                };
                InchiStereo {
                    neighbor: neighbors.map(|index| index as i16),
                    central_atom: center,
                    kind,
                    parity: match parity {
                        Parity::Odd => 1,
                        Parity::Even => 2,
                        Parity::Unknown => 3,
                    },
                }
            })
            .collect();
        let mut options = CString::new(format!("-W{timeout}"))
            .map_err(|_| crate::error(ErrorKind::InvalidInput, "Invalid timeout option."))?
            .into_bytes_with_nul();
        let mut input = InchiInput {
            atom: atoms.as_mut_ptr(),
            stereo: if stereo.is_empty() {
                ptr::null_mut()
            } else {
                stereo.as_mut_ptr()
            },
            options: options.as_mut_ptr().cast(),
            num_atoms: atoms.len() as i16,
            num_stereo: stereo.len() as i16,
        };
        let _lock = NATIVE_CALL.lock().map_err(|_| {
            crate::error(
                ErrorKind::Unavailable,
                "The native InChI serialization lock is poisoned.",
            )
        })?;
        let mut output = OwnedOutput {
            raw: InchiOutput {
                inchi: ptr::null_mut(),
                aux: ptr::null_mut(),
                message: ptr::null_mut(),
                log: ptr::null_mut(),
            },
            free: self.free,
        };
        // SAFETY: input points to initialized, validated mutable arrays whose allocation
        // lives across this synchronous call; options are mutable NUL-terminated bytes.
        // Output is zero-initialized and freed once by its matching FreeStdINCHI function.
        let status = unsafe { (self.generate)(&mut input, &mut output.raw) };
        let inchi = copy_text(output.raw.inchi)?;
        let aux_info = copy_text(output.raw.aux)?;
        let message = copy_text(output.raw.message)?;
        let log = copy_text(output.raw.log)?;
        if status != 0 && status != 1 {
            return Err(Error {
                kind: ErrorKind::Native,
                native_code: Some(status),
                message: format!(
                    "Native InChI returned {status}: {}",
                    message.as_deref().unwrap_or("no message")
                ),
            });
        }
        let inchi = inchi
            .filter(|text| text.starts_with("InChI=1S/"))
            .ok_or_else(|| {
                crate::error(
                    ErrorKind::InvalidOutput,
                    "The native library returned no Standard InChI.",
                )
            })?;
        let inchi_text = CString::new(inchi.as_str())
            .map_err(|_| crate::error(ErrorKind::InvalidOutput, "Invalid native InChI string."))?;
        let mut key = [0 as c_char; 28];
        // SAFETY: inchi_text is a live NUL-terminated Standard InChI and key has the
        // documented minimum 28 bytes. Calls remain serialized and the library remains loaded.
        let key_status = unsafe { (self.key)(inchi_text.as_ptr(), key.as_mut_ptr()) };
        if key_status != 0 {
            return Err(Error {
                kind: ErrorKind::Native,
                native_code: Some(key_status),
                message: format!("Standard InChIKey generation returned {key_status}."),
            });
        }
        let key_bytes: Vec<u8> = key.iter().map(|byte| *byte as u8).collect();
        if key_bytes[27] != 0
            || !key_bytes[..27].iter().enumerate().all(|(index, byte)| {
                if index == 14 || index == 25 {
                    *byte == b'-'
                } else {
                    byte.is_ascii_uppercase()
                }
            })
        {
            return Err(crate::error(
                ErrorKind::InvalidOutput,
                "The native library returned a malformed Standard InChIKey.",
            ));
        }
        let inchikey = String::from_utf8(key_bytes[..27].to_vec()).map_err(|_| {
            crate::error(
                ErrorKind::InvalidOutput,
                "The native InChIKey is not ASCII.",
            )
        })?;
        Ok(Output {
            return_code: status,
            inchi,
            inchikey,
            aux_info,
            message,
            log,
            warning: status == 1,
        })
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        // A poisoned lock means a previous Rust boundary failed unexpectedly. Retain the
        // loaded mapping rather than entering native teardown in that uncertain state.
        if let Ok(_guard) = NATIVE_CALL.lock() {
            // SAFETY: this unique Library owns one successful dlopen reference. Rust
            // borrowing prevents its methods running during Drop; the process lock is held.
            unsafe {
                dlclose(self.handle);
            }
        } else {
            // Retain this descriptor number too, so a later registration cannot reuse
            // the leaked handle's loader pathname after lock poisoning.
            if let Some(file) = self.registered_file.take() {
                mem::forget(file);
            }
        }
    }
}

struct OwnedOutput {
    raw: InchiOutput,
    free: Free,
}
impl Drop for OwnedOutput {
    fn drop(&mut self) {
        // SAFETY: raw is initialized and belongs to exactly one matching GetStdINCHI
        // call; the function pointer is live and the enclosing native-call lock is held.
        unsafe {
            (self.free)(&mut self.raw);
        }
    }
}

fn symbol(handle: *mut c_void, name: &CStr) -> Result<*mut c_void, Error> {
    // SAFETY: handle is live and name is NUL-terminated. dlsym's result is checked
    // before the caller converts it to the exact documented function signature.
    let address = unsafe { dlsym(handle, name.as_ptr()) };
    if address.is_null() {
        Err(crate::error(
            ErrorKind::Unavailable,
            format!(
                "Missing required InChI symbol {}: {}",
                name.to_string_lossy(),
                loader_error()
            ),
        ))
    } else {
        Ok(address)
    }
}

fn loader_error() -> String {
    // SAFETY: dlerror returns null or a loader-owned NUL-terminated message valid
    // until the next loader operation on this thread; copy it immediately.
    let pointer = unsafe { dlerror() };
    if pointer.is_null() {
        "Dynamic InChI library loading failed.".into()
    } else {
        unsafe { CStr::from_ptr(pointer) }
            .to_string_lossy()
            .into_owned()
    }
}

fn copy_text(pointer: *const c_char) -> Result<Option<String>, Error> {
    if pointer.is_null() {
        return Ok(None);
    }
    // SAFETY: these pointers originate solely from the trusted official SDK output;
    // its contract gives NUL-terminated allocations valid until FreeStdINCHI.
    let bytes = unsafe { CStr::from_ptr(pointer) }.to_bytes();
    if bytes.len() > MAX_OUTPUT_BYTES {
        return Err(crate::error(
            ErrorKind::Limit,
            "A native InChI output string exceeds 1 MiB.",
        ));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| {
        crate::error(
            ErrorKind::InvalidOutput,
            "The native InChI output is not UTF-8.",
        )
    })?;
    Ok((!text.is_empty()).then(|| text.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn abi_layout_matches_the_pinned_public_header() {
        assert_eq!(mem::size_of::<InchiAtom>(), 120);
        assert_eq!(mem::offset_of!(InchiAtom, neighbor), 24);
        assert_eq!(mem::offset_of!(InchiAtom, num_bonds), 110);
        assert_eq!(mem::offset_of!(InchiAtom, isotopic_mass), 116);
        assert_eq!(mem::size_of::<InchiStereo>(), 12);
        assert_eq!(
            mem::size_of::<InchiOutput>(),
            4 * mem::size_of::<*mut c_void>()
        );
        assert_eq!(
            mem::offset_of!(InchiInput, num_atoms),
            3 * mem::size_of::<*mut c_void>()
        );
    }
}
