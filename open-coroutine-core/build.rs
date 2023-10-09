fn main() -> std::io::Result<()> {
    cfg_if::cfg_if! {
        if #[cfg(target_os = "linux")] {
            cc::Build::new()
                .warnings(true)
                .file("c_src/version.c")
                .compile("version");
        }
    }
    cfg_if::cfg_if! {
        if #[cfg(any(feature = "korosensei", feature = "boost"))] {
            Ok(())
        } else {
            Err(std::io::Error::new(std::io::ErrorKind::InvalidData,
                "need to enable korosensei or boost feature"))
        }
    }
}
