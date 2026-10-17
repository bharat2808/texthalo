use super::*;

fn filename(text: &str) -> String {
    let name: String = text
        .chars()
        .take(120)
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '"' | '<' | '>' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let name = name.trim().trim_matches('.');
    if name.is_empty() {
        "Audiobook".into()
    } else {
        name.into()
    }
}

fn export_bounds(book: &Book, chapter_id: Option<&str>) -> Result<(u64, u64, String), String> {
    if let Some(id) = chapter_id {
        let (index, checkpoint) = book
            .checkpoints
            .iter()
            .enumerate()
            .find(|(_, c)| c.id == id)
            .ok_or("Chapter not found")?;
        if chapter_view(book, index, checkpoint)["complete"] != true {
            return Err("Only completed chapters can be exported".into());
        }
        let end = book
            .checkpoints
            .get(index + 1)
            .map_or(book.sentences.len(), |c| c.sentence);
        let start = book
            .sentences
            .get(checkpoint.sentence)
            .ok_or("Chapter audio unavailable")?
            .start_sample;
        let last = book
            .sentences
            .get(end.checked_sub(1).ok_or("Chapter is empty")?)
            .ok_or("Chapter audio unavailable")?;
        if last.end_sample <= start {
            return Err("Chapter audio is empty".into());
        }
        Ok((
            start,
            last.end_sample - start,
            format!(
                "{} - {}.wav",
                filename(&book.title),
                filename(&checkpoint.label)
            ),
        ))
    } else {
        if !book.complete
            || !coverage_intact(book)
            || book.generated != book.sentences.len()
            || !book.pending.is_empty()
        {
            return Err("Finish preparing the full audiobook before exporting it".into());
        }
        if book.committed_samples == 0 {
            return Err("Audiobook audio is empty".into());
        }
        Ok((
            0,
            book.committed_samples,
            format!("{}.wav", filename(&book.title)),
        ))
    }
}

fn write_export(
    mut input: fs::File,
    start: u64,
    samples: u64,
    destination: &Path,
) -> Result<(), String> {
    let parent = destination.parent().ok_or("Choose an export folder")?;
    let temporary = parent.join(format!(".texthalo-export-{}.tmp", identity()));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut output = options.open(&temporary).map_err(|e| e.to_string())?;
        output
            .write_all(&header(samples)?)
            .map_err(|e| e.to_string())?;
        input
            .seek(SeekFrom::Start(44 + start * 2))
            .map_err(|e| e.to_string())?;
        if std::io::copy(&mut input.take(samples * 2), &mut output).map_err(|e| e.to_string())?
            != samples * 2
        {
            return Err("Saved audio is incomplete; export was not created".into());
        }
        output.sync_all().map_err(|e| e.to_string())?;
        fs::rename(&temporary, destination).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

/// A missing destination prepares a private copy for the native share menu.
#[tauri::command]
pub async fn audiobook_export(
    app: AppHandle,
    id: String,
    range_id: Option<String>,
    chapter_id: Option<String>,
    destination: Option<String>,
) -> Result<String, String> {
    let service = app.state::<Audiobooks>().inner().clone();
    let path = tauri::async_runtime::spawn_blocking(move || {
        let (input, start, samples, name) = {
            let library = service.0.lock().unwrap();
            let book = library.books.get(&id).ok_or("Audiobook not found")?;
            let root = directory(&id)?;
            let range = if chapter_id.is_some() {
                book.ranges()
                    .find(|b| Some(b.range_id.as_str()) == range_id.as_deref())
                    .ok_or("Saved chapter not found")?
            } else {
                book
            };
            let folder = if book
                .full_scan
                .as_ref()
                .is_some_and(|s| s.draft().range_id == range.range_id)
            {
                book.full_scan.as_ref().unwrap().folder(&root)?
            } else {
                root
            };
            let (start, samples, name) = export_bounds(range, chapter_id.as_deref())?;
            // Open the committed recording while locked; copying can then proceed
            // without blocking playback, polling or generation.
            let input = fs::File::open(audio_path(&folder, range)?).map_err(|e| e.to_string())?;
            (input, start, samples, name)
        };
        let path = if let Some(destination) = destination {
            let path = PathBuf::from(destination);
            if !path.is_absolute() {
                return Err("Choose an absolute export path".into());
            }
            let parent = path
                .parent()
                .ok_or("Choose an export folder")?
                .canonicalize()
                .map_err(|e| e.to_string())?;
            if parent.starts_with(root()?.canonicalize().map_err(|e| e.to_string())?) {
                return Err("Choose a folder outside the internal audiobook library".into());
            }
            path
        } else {
            let folder = root()?.join("exports").join(identity());
            fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
            folder.join(name)
        };
        write_export(input, start, samples, &path)?;
        Ok::<_, String>(path.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(path)
}

#[cfg(target_os = "macos")]
thread_local! {
    // AppKit does not retain the picker for us. Keep the current picker alive.
    static SHARE_PICKER: std::cell::RefCell<Option<objc2::rc::Retained<objc2_app_kit::NSSharingServicePicker>>> = const { std::cell::RefCell::new(None) };
}

#[tauri::command]
pub async fn audiobook_share(window: tauri::WebviewWindow, path: String) -> Result<(), String> {
    let path = PathBuf::from(path)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let exports = root()?
        .join("exports")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !path.starts_with(exports) || !path.is_file() {
        return Err("Prepare an audiobook export before sharing".into());
    }
    #[cfg(target_os = "macos")]
    {
        let (send, receive) = tokio::sync::oneshot::channel();
        let app = window.app_handle().clone();
        app.run_on_main_thread(move || {
            let result = (|| {
                use objc2::{AnyThread, MainThreadMarker};
                use objc2_app_kit::{NSSharingServicePicker, NSWindow};
                use objc2_foundation::{
                    NSArray, NSPoint, NSRect, NSRectEdge, NSSize, NSString, NSURL,
                };
                let _mtm = MainThreadMarker::new().ok_or("Sharing requires the main thread")?;
                let raw = window.ns_window().map_err(|e| e.to_string())?;
                let native = unsafe { &*(raw as *const NSWindow) };
                let view = native.contentView().ok_or("Sharing window unavailable")?;
                let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
                let items = NSArray::<objc2::runtime::AnyObject>::from_slice(&[&url]);
                let picker = unsafe {
                    NSSharingServicePicker::initWithItems(NSSharingServicePicker::alloc(), &items)
                };
                let bounds = view.bounds();
                picker.showRelativeToRect_ofView_preferredEdge(
                    NSRect::new(
                        NSPoint::new(bounds.size.width / 2.0, bounds.size.height / 2.0),
                        NSSize::new(1.0, 1.0),
                    ),
                    &view,
                    NSRectEdge::MinY,
                );
                SHARE_PICKER.with(|slot| {
                    *slot.borrow_mut() = Some(picker);
                });
                Ok::<_, String>(())
            })();
            let _ = send.send(result);
        })
        .map_err(|e| e.to_string())?;
        receive.await.map_err(|e| e.to_string())?
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Err("Use Export to save and share this audio file".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audiobooks::tests::{book, reader};

    #[test]
    fn completed_chapter_exports_exact_pcm_and_full_export_waits_for_completion() {
        let folder = std::env::temp_dir().join(format!("texthalo-export-test-{}", identity()));
        fs::create_dir_all(&folder).unwrap();
        let mut b = book();
        capture(
            &mut b,
            &reader(1, "one"),
            "Chapter One\nThe traveler opened the gate.",
        )
        .unwrap();
        recover(&folder, &b).unwrap();
        commit(&folder, &mut b, &[0.25; 120]).unwrap();
        assert!(export_bounds(&b, Some(&b.checkpoints[0].id)).is_err());
        capture(
            &mut b,
            &reader(2, "two"),
            "Chapter Two\nThe sun rose over the village.",
        )
        .unwrap();
        let (start, samples, name) = export_bounds(&b, Some(&b.checkpoints[0].id)).unwrap();
        assert_eq!((start, samples), (0, 120));
        assert_eq!(name, "Book - Chapter One.wav");
        assert!(export_bounds(&b, None).is_err());
        commit(&folder, &mut b, &[-0.5; 80]).unwrap();
        b.end_verified = true;
        b.complete = true;
        let destination = folder.join("export.wav");
        write_export(
            fs::File::open(audio_path(&folder, &b).unwrap()).unwrap(),
            start,
            samples,
            &destination,
        )
        .unwrap();
        let bytes = fs::read(&destination).unwrap();
        let source = fs::read(audio_path(&folder, &b).unwrap()).unwrap();
        assert_eq!(&bytes[44..], &source[44..44 + 240]);
        assert_eq!(bytes.len(), 44 + 240);
        assert_eq!(export_bounds(&b, None).unwrap().1, 200);
        // A failed copy preserves the existing destination.
        assert!(write_export(
            fs::File::open(audio_path(&folder, &b).unwrap()).unwrap(),
            10000,
            20,
            &destination
        )
        .is_err());
        assert_eq!(fs::read(&destination).unwrap(), bytes);
        let (start, samples, _) = export_bounds(&b, Some(&b.checkpoints[1].id)).unwrap();
        assert_eq!((start, samples), (120, 80));
        fs::remove_dir_all(folder).unwrap();
    }
}
