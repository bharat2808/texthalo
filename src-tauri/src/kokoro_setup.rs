//! Add the pronunciation runtime before building the multilingual voice download plan.
//! A retry preserves completed work: the runtime and model installer both reuse existing files.

pub fn install_languages(
    runtime_ready: bool,
    install_runtime: impl FnOnce() -> Result<(), String>,
    install_voice_files: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    if !runtime_ready {
        install_runtime()?;
    }
    install_voice_files()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn installs_runtime_before_discovering_and_downloading_voice_files() {
        let steps = RefCell::new(Vec::new());
        install_languages(
            false,
            || {
                steps.borrow_mut().push("runtime");
                Ok(())
            },
            || {
                steps.borrow_mut().push("voice files");
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(*steps.borrow(), ["runtime", "voice files"]);
    }

    #[test]
    fn an_existing_runtime_still_repairs_missing_voice_files() {
        let steps = RefCell::new(Vec::new());
        install_languages(
            true,
            || panic!("existing runtime must not be installed again"),
            || {
                steps.borrow_mut().push("voice files");
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(*steps.borrow(), ["voice files"]);
    }

    #[test]
    fn runtime_failure_stops_before_voice_download_and_retry_preserves_runtime() {
        assert_eq!(
            install_languages(
                false,
                || Err("runtime offline".into()),
                || panic!("runtime is required")
            ),
            Err("runtime offline".into())
        );
        assert_eq!(
            install_languages(
                true,
                || panic!("runtime remains installed"),
                || Err("voice download offline".into())
            ),
            Err("voice download offline".into())
        );
        install_languages(true, || panic!("retry must reuse runtime"), || Ok(())).unwrap();
    }
}
