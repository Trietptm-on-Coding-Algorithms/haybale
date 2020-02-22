use crate::project::Project;

/// Enum used for the `demangling` option in `Config`.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum Demangling {
    /// Don't try to demangle
    NoDemangling,
    /// Try to demangle using the C++ demangler (suitable for `Project`s containing C++ code).
    /// Names that fail to demangle will simply be printed as-is.
    CPP,
    /// Try to demangle using the Rust demangler (suitable for `Project`s containing Rust code).
    /// Names that fail to demangle will simply be printed as-is.
    Rust,
}

impl Demangling {
    /// Attempts to demangle the given function name, as appropriate based on the
    /// `Demangling` setting.
    //
    // (takes `self` by value because `self` is `Copy`)
    pub fn maybe_demangle(self, funcname: &str) -> String {
        match self {
            Demangling::NoDemangling => funcname.to_owned(),
            Demangling::CPP => cpp_demangle_or_id(funcname),
            Demangling::Rust => rust_demangle_or_id(funcname),
        }
    }

    /// Guesses an appropriate `Demangling` for the given `Project`.
    pub fn autodetect(proj: &Project) -> Self {
        // our autodetection is pretty unsophisticated right now,
        // but something is better than nothing

        // if any file in the `Project` comes from a source file
        // ending in `.rs`, then use Rust demangling.
        // Empirically, bitcode generated by `rustc` may have a "source
        // filename" ending in `cgu.0` instead (for example, our test file
        // `panic.rs` is compiled to a bitcode file with a "source filename"
        // of `panic.3a1fbbbh-cgu.0`), so also check for filenames ending in
        // `u.0`. (False positives aren't the end of the world, because any
        // symbols that aren't actually valid Rust symbols will be passed
        // through the demangler unchanged.)
        // TODO figure out a tighter test here, hopefully we can avoid both
        // false positives and false negatives.
        if proj.module_source_file_names().any(|name| name.ends_with(".rs") || name.ends_with("u.0")) {
            return Demangling::Rust;
        }

        // otherwise, if any file in the `Project` comes from a source
        // file ending in `.cpp`, then use C++ demangling
        if proj.module_source_file_names().any(|name| name.ends_with(".cpp")) {
            return Demangling::CPP;
        }

        // otherwise give up and don't try to demangle
        Demangling::NoDemangling
    }
}

/// Helper function to demangle function names with the C++ demangler.
///
/// Returns `Some` if successfully demangled, or `None` if any error occurs
/// (for instance, if `funcname` isn't a valid C++ mangled name)
pub(crate) fn try_cpp_demangle(funcname: &str) -> Option<String> {
    let opts = cpp_demangle::DemangleOptions {
        no_params: true,
    };
    cpp_demangle::Symbol::new(funcname).ok().and_then(|sym| sym.demangle(&opts).ok())
}

/// Like `try_cpp_demangle()`, but just returns the input string unmodified in
/// the case of any error, rather than returning `None`.
pub(crate) fn cpp_demangle_or_id(funcname: &str) -> String {
    try_cpp_demangle(funcname).unwrap_or_else(|| funcname.to_owned())
}

/// Helper function to demangle function names with the Rust demangler.
///
/// Returns `Some` if successfully demangled, or `None` if any error occurs
/// (for instance, if `funcname` isn't a valid Rust mangled name)
pub(crate) fn try_rust_demangle(funcname: &str) -> Option<String> {
    rustc_demangle::try_demangle(funcname).ok().map(|demangled|
        format!("{:#}", demangled)
    )
}

/// Like `try_rust_demangle()`, but just returns the input string unmodified in
/// the case of any error, rather than returning `None`.
pub(crate) fn rust_demangle_or_id(funcname: &str) -> String {
    format!("{:#}", rustc_demangle::demangle(funcname))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn autodetect() -> Result<(), String> {
        // A `Project` from a single C file
        let c_proj = Project::from_bc_path(&Path::new("tests/bcfiles/basic.bc"))?;
        assert_eq!(Demangling::autodetect(&c_proj), Demangling::NoDemangling);

        // A `Project` from a single C++ file
        let cpp_proj = Project::from_bc_path(&Path::new("tests/bcfiles/throwcatch.bc"))?;
        assert_eq!(Demangling::autodetect(&cpp_proj), Demangling::CPP);

        // A `Project` from a single Rust file
        let rust_proj = Project::from_bc_path(&Path::new("tests/bcfiles/panic.bc"))?;
        assert_eq!(Demangling::autodetect(&rust_proj), Demangling::Rust);

        // A `Project` containing multiple C files
        let c_proj = Project::from_bc_paths(vec![
            &Path::new("tests/bcfiles/basic.bc"),
            &Path::new("tests/bcfiles/call.bc"),
            &Path::new("tests/bcfiles/globals.bc"),
            &Path::new("tests/bcfiles/simd.bc"),
        ])?;
        assert_eq!(Demangling::autodetect(&c_proj), Demangling::NoDemangling);

        // A `Project` containing both C and Rust files
        let c_rust_proj = Project::from_bc_paths(vec![
            &Path::new("tests/bcfiles/basic.bc"),
            &Path::new("tests/bcfiles/call.bc"),
            &Path::new("tests/bcfiles/panic.bc"),
        ])?;
        assert_eq!(Demangling::autodetect(&c_rust_proj), Demangling::Rust);

        // A `Project` containing both C and C++ files
        let c_cpp_proj = Project::from_bc_paths(vec![
            &Path::new("tests/bcfiles/basic.bc"),
            &Path::new("tests/bcfiles/call.bc"),
            &Path::new("tests/bcfiles/throwcatch.bc"),
        ])?;
        assert_eq!(Demangling::autodetect(&c_cpp_proj), Demangling::CPP);

        Ok(())
    }
}