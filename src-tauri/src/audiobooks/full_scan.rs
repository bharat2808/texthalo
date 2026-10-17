//! Complete books are rebuilt from verified page turns, never from a cached tail hint.
//! The old recording remains playable until the replacement is atomically published.
use super::*;
use std::collections::HashMap;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct FullScan {
    draft: Box<Book>,
    sources: Vec<Book>,
    phase: String,
    reused: usize,
    #[serde(default)]
    reuse_policy: u8,
    #[serde(skip)]
    cache: HashMap<String, Vec<(usize, usize)>>,
}
fn canonical(text: &str) -> String {
    text_words(text)
        .into_iter()
        .map(|w| w.key)
        .collect::<Vec<_>>()
        .join(" ")
}
fn reusable_sentence(book: &Book, sentence: usize) -> bool {
    book.checkpoints
        .iter()
        .enumerate()
        .rev()
        .find(|(_, c)| c.sentence <= sentence)
        .is_some_and(|(i, c)| chapter_view(book, i, c)["complete"] == true)
}
// A changed DOM fingerprint can arrive before the newly rendered page pixels.
// Re-read that same view rather than navigating again or committing old text.
fn read_fresh_view_with(
    command: impl Fn(Value) -> Result<Value, String>,
    active: impl Fn() -> bool,
    book: &Book,
    mut reader: Value,
    _verified_forward: bool,
) -> Result<Value, String> {
    for attempt in 0..4 {
        if !active() {
            return Err("Preparation stopped".into());
        }
        if key(&reader)? != book.key {
            return Err("Connected book changed".into());
        }
        let result = read_view_with(
            &command,
            json!({"type":"ocr","bookId":reader["bookId"],"fingerprint":reader["fingerprint"]}),
        )?;
        if result["reader"]["fingerprint"] != reader["fingerprint"]
            || key(&result["reader"])? != book.key
        {
            return Err("Reader moved while verifying full audiobook coverage".into());
        }
        let normal = result["text"]
            .as_str()
            .ok_or("Page text unavailable")?
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let suspect = repeated_capture(book, &reader, &normal, false);
        if !suspect {
            return Ok(result);
        }
        if attempt < 3 {
            std::thread::sleep(std::time::Duration::from_millis(250));
            let next = command(json!({"type":"inspect"}))?;
            // Retry pixel rendering, never follow a user-selected different view.
            if next["fingerprint"] != reader["fingerprint"] {
                return Err("Reader changed while retrying a stale page capture. Resume preparation from the saved view.".into());
            }
            reader = next;
        }
    }
    Err("The reader is still showing a previously captured view. No text was skipped. Wait for the page to finish loading, then resume full audiobook preparation.".into())
}
fn same_passage(source: &Book, old: &Sentence, draft: &Book, new: &Sentence) -> bool {
    let context = |page: &Page, source: &Source| {
        let words = text_words(&page.text);
        let start = words
            .iter()
            .position(|w| w.end > source.start)
            .unwrap_or(words.len());
        let end = words
            .iter()
            .rposition(|w| w.start < source.end)
            .map_or(start, |i| i + 1);
        words[start.saturating_sub(8)..(end + 8).min(words.len())]
            .iter()
            .map(|w| w.key.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    };
    for old_source in &old.sources {
        let Some(old_page) = source.pages.get(old_source.page) else {
            continue;
        };
        for new_source in &new.sources {
            let Some(new_page) = draft.pages.get(new_source.page) else {
                continue;
            };
            let same_position = ReaderPosition::parse(&old_page.progress)
                .zip(ReaderPosition::parse(&new_page.progress))
                .is_some_and(|(a, b)| a.compare(b) == Some(std::cmp::Ordering::Equal));
            if !same_position {
                continue;
            }
            // A sentence recorded only in the later half of a view must not match
            // an earlier repeated sentence when we rescan the complete view.
            if old_page.text == new_page.text {
                if old_source.start == new_source.start && old_source.end == new_source.end {
                    return true;
                }
                continue;
            }
            let old_context = context(old_page, old_source);
            let new_context = context(new_page, new_source);
            if text_words(&old_context).len() >= 12
                && page_matches(&old_context, &new_context).unwrap_or(false)
            {
                return true;
            }
        }
    }
    false
}
// A full scan can join a page's imported sentence fragment to the preceding
// page. Reconcile that bookmark through its captured source span, not text alone.
fn same_bookmark_passage(old_book: &Book, old: &Sentence, new_book: &Book, new: &Sentence) -> bool {
    if same_passage(old_book, old, new_book, new) {
        return true;
    }
    old.sources.iter().any(|a| {
        let Some(old_page) = old_book.pages.get(a.page) else {
            return false;
        };
        new.sources.iter().any(|b| {
            let Some(new_page) = new_book.pages.get(b.page) else {
                return false;
            };
            ReaderPosition::parse(&old_page.progress)
                .zip(ReaderPosition::parse(&new_page.progress))
                .is_some_and(|(x, y)| x.compare(y) == Some(std::cmp::Ordering::Equal))
                && old_page.text == new_page.text
                && b.start <= a.start
                && b.end >= a.end
        })
    })
}
// A successful OCR pass with zero observations is different from a capture error.
// Confirm it twice at the exact same view before retaining an image-only/blank page.
fn read_view_with(
    command: impl Fn(Value) -> Result<Value, String>,
    request: Value,
) -> Result<Value, String> {
    const EMPTY: &str = "No readable text found in the page crop";
    match command(request.clone()) {
        Err(error) if error == EMPTY => {
            let before = command(json!({"type":"inspect"}))?;
            if before["fingerprint"] != request["fingerprint"]
                || before["bookId"] != request["bookId"]
            {
                return Err("Reader moved while checking a page without readable text".into());
            }
            match command(request.clone()) {
                Err(error) if error == EMPTY => {
                    let after = command(json!({"type":"inspect"}))?;
                    if after["fingerprint"] != before["fingerprint"]
                        || key(&after)? != key(&before)?
                    {
                        return Err(
                            "Reader moved while checking a page without readable text".into()
                        );
                    }
                    Ok(json!({"reader":after,"text":"","emptyConfirmed":true}))
                }
                result => result,
            }
        }
        result => result,
    }
}
fn restore_scan_view_with(
    command: impl Fn(Value) -> Result<Value, String>,
    active: impl Fn() -> bool,
    book: &Book,
    target: &Page,
) -> Result<Value, String> {
    let goal = ReaderPosition::parse(&target.progress)
        .ok_or("Saved preparation page or location unavailable")?;
    let mut r = command(json!({"type":"inspect"}))?;
    let initial = ReaderPosition::parse(r["progress"].as_str().unwrap_or(""))
        .ok_or("Reader page or location unavailable")?;
    let started_equal = initial.compare(goal) == Some(std::cmp::Ordering::Equal);
    let mut direction = match initial.compare(goal) {
        Some(std::cmp::Ordering::Less) => "next",
        Some(_) => "previous",
        None if matches!(goal, ReaderPosition::Location(1)) => "previous",
        None if r["startOfBook"] == true && matches!(initial, ReaderPosition::Location(1)) => {
            "next"
        }
        None if r["fingerprint"].as_str() == Some(&target.fingerprint) => "next",
        None => return Err("Open the saved preparation page to resume".into()),
    };
    let mut reversed = false;
    let mut visited = std::collections::HashSet::new();
    for _ in 0..10_000 {
        if !active() {
            return Err("Preparation stopped".into());
        }
        if key(&r)? != book.key {
            return Err("Connected book changed".into());
        }
        let comparison = ReaderPosition::parse(r["progress"].as_str().unwrap_or(""))
            .and_then(|at| at.compare(goal));
        let same_fingerprint = r["fingerprint"].as_str() == Some(&target.fingerprint);
        let crossed = (direction == "next" && comparison == Some(std::cmp::Ordering::Greater))
            || (direction == "previous" && comparison == Some(std::cmp::Ordering::Less));
        let boundary = r[if direction == "next" {
            "endOfBook"
        } else {
            "startOfBook"
        }] == true;
        if same_fingerprint || comparison == Some(std::cmp::Ordering::Equal) || crossed || boundary
        {
            let result = read_view_with(
                &command,
                json!({"type":"ocr","bookId":r["bookId"],"fingerprint":r["fingerprint"]}),
            )?;
            if result["reader"]["fingerprint"] != r["fingerprint"]
                || key(&result["reader"])? != book.key
            {
                return Err("Reader moved while restoring preparation".into());
            }
            let text = result["text"]
                .as_str()
                .ok_or("No page text returned")?
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let matches = if target.text.is_empty() {
                same_fingerprint && text.is_empty() && result["emptyConfirmed"] == true
            } else {
                // The entire saved view must agree before advancing. A matching
                // prefix or tail alone can skip text after a reader layout change.
                canonical(&target.text) == canonical(&text)
            };
            if matches {
                return Ok(result["reader"].clone());
            }
            if crossed || boundary {
                // When starting inside repeated page labels, the saved view can
                // lie on either side. Search the other side once, without oscillating.
                if started_equal && !reversed {
                    direction = if direction == "next" {
                        "previous"
                    } else {
                        "next"
                    };
                    reversed = true;
                } else {
                    break;
                }
            }
        }
        if !visited.insert((direction, r["fingerprint"].to_string())) {
            break;
        }
        r = command(
            json!({"type":"navigate","bookId":r["bookId"],"fingerprint":r["fingerprint"],"direction":direction}),
        )?;
    }
    Err(
        "The saved preparation view could not be verified. Restore its layout and page to resume."
            .into(),
    )
}
pub(super) fn recover_preparation(root: &Path, book: &mut Book) {
    let problem = book
        .full_scan
        .as_mut()
        .and_then(|scan| scan.recover(root).err());
    if let Some(error) = problem {
        book.full_scan = None;
        book.message=Some(format!("Full audiobook preparation needs to restart: {error}. Your original recording is available."));
        let _ = save(root, book);
    }
}
impl FullScan {
    pub(super) fn draft(&self) -> &Book {
        &self.draft
    }
    pub(super) fn draft_mut(&mut self) -> &mut Book {
        &mut self.draft
    }
    fn preserve_listening(&mut self, live: &FullScan) {
        if self.draft.range_id == live.draft.range_id {
            self.draft.position = live.draft.position;
            self.draft.speed = live.draft.speed;
            self.draft.bookmark_sentence = live.draft.bookmark_sentence.clone();
            self.draft.bookmark_offset = live.draft.bookmark_offset;
        }
    }
    fn new(book: &Book) -> Self {
        let sources = std::iter::once(book)
            .chain(book.saved_ranges.iter().map(|b| b.as_ref()))
            .map(|b| {
                let mut b = b.clone();
                b.saved_ranges.clear();
                b.full_scan = None;
                b
            })
            .collect();
        let mut draft = book.clone();
        draft.range_id = identity();
        draft.audio_file = default_audio();
        draft.saved_ranges.clear();
        draft.full_scan = None;
        draft.pages.clear();
        draft.sentences.clear();
        draft.checkpoints.clear();
        draft.pending.clear();
        draft.generated = 0;
        draft.committed_samples = 0;
        draft.bookmark_sentence = None;
        draft.bookmark_offset = 0;
        draft.position = 0.;
        draft.complete = false;
        draft.from_beginning = false;
        draft.end_verified = false;
        draft.continuous = false;
        draft.pending_start = None;
        draft.reader_hint = None;
        draft.recovery = None;
        draft.message = None;
        draft.activity = None;
        let mut scan = Self {
            draft: Box::new(draft),
            sources,
            phase: "finding-start".into(),
            reused: 0,
            reuse_policy: 1,
            cache: HashMap::new(),
        };
        scan.index();
        scan
    }
    fn index(&mut self) {
        self.cache.clear();
        for (i, source) in self.sources.iter().enumerate() {
            if recording_key(source) != recording_key(&self.draft) || source.key != self.draft.key {
                continue;
            }
            for (j, sentence) in source.sentences.iter().take(source.generated).enumerate() {
                if !reusable_sentence(source, j) || sentence.end_sample <= sentence.start_sample {
                    continue;
                }
                let key = canonical(&sentence.text);
                if key.is_empty() {
                    continue;
                }
                let entries = self.cache.entry(key).or_default();
                if !entries
                    .iter()
                    .any(|&(a, b)| self.sources[a].sentences[b].id == sentence.id)
                {
                    entries.push((i, j));
                }
            }
        }
    }
    pub(super) fn folder(&self, root: &Path) -> Result<PathBuf, String> {
        uuid::Uuid::parse_str(&self.draft.range_id)
            .map_err(|_| "Invalid full audiobook identity")?;
        Ok(root.join(format!("full-{}", self.draft.range_id)))
    }
    pub(super) fn recover(&mut self, root: &Path) -> Result<(), String> {
        let folder = self.folder(root)?;
        fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        if let Ok(bytes) = fs::read(folder.join("book.json")) {
            let saved: Book = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if saved.id != self.draft.id
                || saved.range_id != self.draft.range_id
                || saved.full_scan.is_some()
            {
                return Err("Invalid full audiobook progress".into());
            }
            *self.draft = saved;
        }
        recover(&folder, &self.draft)?;
        load_bookmark(&folder, &mut self.draft);
        if repair_ocr_chapter_boundaries(&mut self.draft) {
            save(&folder, &self.draft)?;
        }
        if self.reuse_policy == 0 {
            let reused_partial = self
                .draft
                .sentences
                .iter()
                .take(self.draft.generated)
                .any(|s| {
                    self.sources.iter().any(|b| {
                        b.sentences
                            .iter()
                            .enumerate()
                            .any(|(i, old)| old.id == s.id && !reusable_sentence(b, i))
                    })
                });
            if reused_partial {
                let mut original = self
                    .sources
                    .first()
                    .cloned()
                    .ok_or("Original audiobook unavailable")?;
                original.saved_ranges =
                    self.sources.iter().skip(1).cloned().map(Box::new).collect();
                *self = Self::new(&original);
                return self.recover(root);
            }
            self.reuse_policy = 1;
        }
        self.phase = if self.transcript_ready() {
            "rendering"
        } else if self.draft.pages.is_empty() {
            "finding-start"
        } else {
            "scanning"
        }
        .into();
        self.index();
        self.reused = self
            .draft
            .sentences
            .iter()
            .take(self.draft.generated)
            .filter(|s| {
                self.sources.iter().any(|b| {
                    b.sentences
                        .iter()
                        .take(b.generated)
                        .any(|old| old.id == s.id)
                })
            })
            .count();
        Ok(())
    }
    pub(super) fn transcript_ready(&self) -> bool {
        self.draft.from_beginning
            && self.draft.end_verified
            && self.draft.pending.is_empty()
            && !self.draft.pages.is_empty()
    }
    fn save_transcript(&self, root: &Path) -> Result<(), String> {
        if !self.transcript_ready() {
            return Err("The complete transcript has not been captured".into());
        }
        let chapters = self
            .draft
            .checkpoints
            .iter()
            .enumerate()
            .map(|(i, chapter)| {
                json!({"id":chapter.id,"label":chapter.label,"verifiedStart":chapter.verified_start,
                "startSentence":chapter.sentence,"endSentence":self.draft.checkpoints.get(i+1)
                    .map_or(self.draft.sentences.len(), |next| next.sentence)})
            })
            .collect::<Vec<_>>();
        let transcript = json!({"version":1,"bookId":self.draft.book_id,"title":self.draft.title,
            "complete":true,"pages":self.draft.pages,"chapters":chapters,
            "text":self.draft.pages.iter().map(|p|p.text.as_str()).collect::<Vec<_>>().join("\n\n")});
        let folder = self.folder(root)?;
        let temporary = folder.join("transcript.tmp");
        let mut output = fs::File::create(&temporary).map_err(|e| e.to_string())?;
        output
            .write_all(transcript.to_string().as_bytes())
            .map_err(|e| e.to_string())?;
        output.sync_all().map_err(|e| e.to_string())?;
        fs::rename(temporary, folder.join("transcript.json")).map_err(|e| e.to_string())
    }
    pub(super) fn view(&self) -> Value {
        json!({"chapters":self.draft.checkpoints.len(),"completedChapters":self.draft.checkpoints.iter().enumerate().filter(|(i,c)|chapter_view(&self.draft,*i,c)["complete"]==true).count(),"transcriptReady":self.transcript_ready(),"total":self.draft.sentences.len(),"phase":self.phase,"pages":self.draft.pages.len(),"page":self.draft.pages.last().map(|p|&p.progress),"generated":self.draft.generated,"reused":self.reused,"pageReady":self.draft.generated,"pageTotal":self.draft.sentences.len()})
    }
    fn capture(&mut self, reader: &Value, text: &str, verified_turn: bool) -> Result<(), String> {
        if key(reader)? != self.draft.key {
            return Err("Connected book changed".into());
        }
        if self.draft.pages.is_empty() {
            if reader["startOfBook"] != true {
                return Err("The beginning of the book could not be verified".into());
            }
            self.draft.from_beginning = true;
        } else if !verified_turn {
            return Err("A continuous page turn is required for full audiobook coverage".into());
        }
        let previous_checkpoints = self.draft.checkpoints.len();
        if text.trim().is_empty() {
            let fingerprint = reader["fingerprint"]
                .as_str()
                .ok_or("Missing page identity")?;
            if self
                .draft
                .pages
                .iter()
                .any(|p| p.fingerprint == fingerprint)
            {
                return Err(
                    "Reader repeated a view while verifying full audiobook coverage".into(),
                );
            }
            self.draft.pages.push(Page {
                heading: None,
                progress: reader["progress"].as_str().unwrap_or("").into(),
                fingerprint: fingerprint.into(),
                text: String::new(),
            });
        } else {
            capture_from_mode(&mut self.draft, reader, text, 0, verified_turn)?;
        }
        // Keep checkpoint identities where the original chapter label is unique.
        for checkpoint in self.draft.checkpoints.iter_mut().skip(previous_checkpoints) {
            let matches = self
                .sources
                .iter()
                .flat_map(|b| b.checkpoints.iter())
                .filter(|c| canonical(&c.label) == canonical(&checkpoint.label))
                .collect::<Vec<_>>();
            if matches.len() == 1 {
                checkpoint.id = matches[0].id.clone();
            }
        }
        if reader["endOfBook"] == true {
            assemble(&mut self.draft, true)?;
            self.draft.end_verified = true;
        }
        self.phase = if self.transcript_ready() {
            "rendering"
        } else {
            "scanning"
        }
        .into();
        Ok(())
    }
    fn reuse(&mut self, root: &Path) -> Result<bool, String> {
        let Some(pending) = self.draft.sentences.get(self.draft.generated) else {
            return Ok(false);
        };
        let Some(entries) = self.cache.get(&canonical(&pending.text)) else {
            return Ok(false);
        };
        if entries.len() != 1 {
            return Ok(false);
        }
        let (i, j) = entries[0];
        let source = &self.sources[i];
        let sentence = &source.sentences[j];
        if !same_passage(source, sentence, &self.draft, pending) {
            return Ok(false);
        }
        // Repeated passages must not create duplicate sentence IDs or move a bookmark.
        if self
            .draft
            .sentences
            .iter()
            .take(self.draft.generated)
            .any(|s| s.id == sentence.id)
        {
            return Ok(false);
        }
        let mut input = fs::File::open(audio_path(root, source)?).map_err(|e| e.to_string())?;
        input
            .seek(SeekFrom::Start(44 + sentence.start_sample * 2))
            .map_err(|e| e.to_string())?;
        let length = usize::try_from((sentence.end_sample - sentence.start_sample) * 2)
            .map_err(|_| "Saved sentence is too large")?;
        let mut pcm = vec![0; length];
        input.read_exact(&mut pcm).map_err(|e| e.to_string())?;
        self.draft.sentences[self.draft.generated].id = sentence.id.clone();
        let voice = if sentence.voice_key.is_empty() {
            format!("kokoro:{}", source.voice.voice)
        } else {
            sentence.voice_key.clone()
        };
        self.draft.sentences[self.draft.generated].voice_key = voice.clone();
        self.draft.sentences[self.draft.generated].voice_label = sentence.voice_label.clone();
        commit_pcm(&self.folder(root)?, &mut self.draft, &pcm, Some(&voice))?;
        self.reused += 1;
        Ok(true)
    }
    fn replacement(&self, current: &Book) -> Result<Book, String> {
        if !self.draft.from_beginning
            || !self.draft.end_verified
            || self.draft.generated != self.draft.sentences.len()
            || !self.draft.pending.is_empty()
        {
            return Err("The full audiobook still has unverified pages or unfinished audio".into());
        }
        let mut next = (*self.draft).clone();
        next.continuous = true;
        next.complete = true;
        next.speed = current.speed;
        next.bookmark_sentence = current.bookmark_sentence.clone();
        next.bookmark_offset = current.bookmark_offset;
        if current.generated > 0
            && next.bookmark_sentence.is_some()
            && stable_position(&next).is_none()
        {
            let target = current
                .sentences
                .iter()
                .find(|s| Some(&s.id) == current.bookmark_sentence.as_ref())
                .ok_or("Saved listening sentence unavailable")?;
            let candidates = next
                .sentences
                .iter()
                .filter(|s| {
                    let old = canonical(&target.text);
                    let new = canonical(&s.text);
                    let contains_fragment = format!(" {new} ").contains(&format!(" {old} "));
                    !old.is_empty()
                        && contains_fragment
                        && same_bookmark_passage(current, target, &next, s)
                })
                .collect::<Vec<_>>();
            if candidates.len() == 1 {
                next.bookmark_sentence = Some(candidates[0].id.clone());
                next.bookmark_offset = 0;
            } else {
                // Do not replace the only usable recording with an uncertain bookmark.
                return Err("The full audiobook is prepared, but the saved listening sentence could not be verified. Your original recording remains available.".into());
            }
        }
        next.position = stable_position(&next).unwrap_or(0.);
        next.message = None;
        next.activity = None;
        next.full_scan = None;
        Ok(next)
    }
    fn publish(&self, root: &Path, current: &mut Book) -> Result<(), String> {
        let mut next = self.replacement(current)?;
        next.audio_file = format!("{}.wav", identity());
        let output = audio_path(root, &next)?;
        fs::copy(audio_path(&self.folder(root)?, &self.draft)?, &output)
            .map_err(|e| e.to_string())?;
        fs::File::open(&output)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        let result = save(root, &next);
        let published = fs::read(root.join("book.json"))
            .ok()
            .and_then(|v| serde_json::from_slice::<Book>(&v).ok())
            .is_some_and(|b| b.audio_file == next.audio_file);
        if published {
            *current = next;
            let _ = save_bookmark(root, current);
            Ok(())
        } else {
            let _ = fs::remove_file(output);
            result.and(Err("Full audiobook was not published".into()))
        }
    }
}
fn persist(
    service: &Audiobooks,
    id: &str,
    token: &str,
    root: &Path,
    scan: &FullScan,
) -> Result<(), String> {
    let mut l = service.0.lock().unwrap();
    if !l
        .producing
        .as_ref()
        .is_some_and(|(i, t, _)| i == id && t == token)
    {
        return Err("Preparation stopped".into());
    }
    let book = l.books.get_mut(id).ok_or("Audiobook removed")?;
    let mut updated = scan.clone();
    if let Some(live) = &book.full_scan {
        updated.preserve_listening(live);
    }
    save(&updated.folder(root)?, &updated.draft)?;
    let mut next = book.clone();
    next.full_scan = Some(Box::new(updated));
    next.activity = Some("full-scan".into());
    save(root, &next)?;
    *book = next;
    if let Some(playing) = l
        .playing
        .as_ref()
        .filter(|p| p.book == id && p.range_id == scan.draft.range_id)
    {
        let audio = playing.player.snapshot();
        if let Some((_, end, _)) = saved_playback(&scan.draft, Some(audio.start)) {
            if end > (audio.buffered * RATE as f64).round() as u64 {
                playing.player.update_file(
                    &audio_path(&scan.folder(root)?, &scan.draft)?,
                    end,
                    true,
                )?;
            }
        }
    }
    Ok(())
}
fn prepare_transcript_with(
    scan: &mut FullScan,
    root: &Path,
    command: impl Fn(Value) -> Result<Value, String>,
    active: impl Fn() -> bool,
    mut checkpoint: impl FnMut(&FullScan) -> Result<(), String>,
) -> Result<(), String> {
    // A verified transcript makes all later rendering independent of Chrome.
    if scan.transcript_ready() {
        scan.save_transcript(root)?;
        return Ok(());
    }
    let mut reader = command(json!({"type":"inspect"}))?;
    if key(&reader)? != scan.draft.key {
        return Err("Connected book changed".into());
    }
    if scan.draft.pages.is_empty() {
        let mut visited = std::collections::HashSet::new();
        for _ in 0..10_000 {
            if !active() {
                return Err("Preparation stopped".into());
            }
            if reader["startOfBook"] == true {
                break;
            }
            if !visited.insert(reader["fingerprint"].to_string()) {
                return Err("Reader repeated a view while finding the book beginning".into());
            }
            reader = command(json!({"type":"navigate","bookId":reader["bookId"],
                "fingerprint":reader["fingerprint"],"direction":"previous"}))?;
            if key(&reader)? != scan.draft.key {
                return Err("Connected book changed".into());
            }
        }
        if reader["startOfBook"] != true {
            return Err("The beginning of the book could not be verified".into());
        }
    } else {
        reader = restore_scan_view_with(
            |request| {
                if request["type"] == "ocr" {
                    read_view_with(&command, request)
                } else {
                    command(request)
                }
            },
            &active,
            &scan.draft,
            scan.draft.pages.last().unwrap(),
        )?;
    }
    let mut need_capture = scan.draft.pages.is_empty();
    let mut verified_turn = false;
    while !scan.transcript_ready() {
        if !active() {
            return Err("Preparation stopped".into());
        }
        if !need_capture {
            scan.phase = "scanning".into();
            checkpoint(scan)?;
            reader = command(json!({"type":"navigate","bookId":reader["bookId"],
                "fingerprint":reader["fingerprint"],"direction":"next"}))?;
            verified_turn = reader["navigationVerified"] == true;
        }
        if scan.draft.pages.len() >= 10_000 {
            return Err("This recording reached the 10,000-page safety limit".into());
        }
        let result = read_fresh_view_with(
            &command,
            &active,
            &scan.draft,
            reader.clone(),
            verified_turn,
        )?;
        if result["reader"]["fingerprint"] != reader["fingerprint"]
            || key(&result["reader"])? != scan.draft.key
        {
            return Err("Reader moved while verifying full audiobook coverage".into());
        }
        let text = result["text"].as_str().ok_or("Page text unavailable")?;
        if (text.trim().is_empty() && result["emptyConfirmed"] != true)
            || text.chars().count() > 20_000
        {
            return Err("Page has no supported passage".into());
        }
        scan.capture(&result["reader"], text, verified_turn)?;
        reader = result["reader"].clone();
        need_capture = false;
        checkpoint(scan)?;
    }
    scan.save_transcript(root)?;
    checkpoint(scan)
}

pub(super) fn producer(app: &AppHandle, id: &str, token: &str) -> Result<(), String> {
    let service = app.state::<Audiobooks>();
    let bridge = app.state::<crate::reader_bridge::ReaderBridge>();
    let root = directory(id)?;
    let book = service
        .0
        .lock()
        .unwrap()
        .books
        .get(id)
        .cloned()
        .ok_or("Audiobook not found")?;
    let settings = app
        .state::<crate::AppState>()
        .settings
        .lock()
        .unwrap()
        .clone();
    let requested_key = synthesis_key(&settings);
    let mut scan = book
        .full_scan
        .as_deref()
        .filter(|scan| scan.draft().synthesis_key == requested_key)
        .cloned()
        .unwrap_or_else(|| FullScan::new(&book));
    scan.draft.synthesis_key = requested_key.clone();
    scan.draft.voice = settings.kokoro.clone();
    scan.index();
    scan.recover(&root)?;
    persist(&service, id, token, &root, &scan)?;
    prepare_transcript_with(
        &mut scan,
        &root,
        |request| {
            bridge.command_while(request, || {
                generation_voice_active(app, &service, id, token, &requested_key)
            })
        },
        || generation_voice_active(app, &service, id, token, &requested_key),
        |scan| persist(&service, id, token, &root, scan),
    )?;
    loop {
        if !generation_active(&service, id, token) {
            return Ok(());
        }
        if !generation_voice_active(app, &service, id, token, &requested_key) {
            return Err(
                "Voice settings changed during generation. Resume to use the newly selected voice."
                    .into(),
            );
        }
        if settings.engine == crate::config::Engine::Fish {
            if let Some(count) = next_fish_batch_len(&scan.draft, true) {
                coalesce_fish_batch(&mut scan.draft, count);
                if count > 1 {
                    persist(&service, id, token, &root, &scan)?;
                }
            }
        }
        if let Some(sentence) = scan.draft.sentences.get(scan.draft.generated).cloned() {
            if !scan.reuse(&root)? {
                let folder = scan.folder(&root)?;
                let scratch = folder.join("sentence.tmp.wav");
                let samples = render_sentence(
                    app,
                    &settings,
                    &sentence.text,
                    narration_heading(&scan.draft),
                    &scratch,
                    || generation_voice_active(app, &service, id, token, &requested_key),
                )?;
                if !generation_voice_active(app, &service, id, token, &requested_key) {
                    return Err("Voice settings changed during generation. Resume to use the newly selected voice.".into());
                }
                commit_with_voice(
                    &folder,
                    &mut scan.draft,
                    &samples,
                    &voice_key(&settings),
                    &voice_label(&settings),
                )?;
            }
            persist(&service, id, token, &root, &scan)?;
            continue;
        }
        if scan.draft.end_verified {
            scan.phase = "publishing".into();
            persist(&service, id, token, &root, &scan)?;
            let mut l = service.0.lock().unwrap();
            if !l.producing.as_ref().is_some_and(|(_, t, _)| t == token) {
                return Ok(());
            }
            let playing = l
                .playing
                .as_ref()
                .filter(|p| p.book == id)
                .map(|p| (p.player.clone(), p.range_id.clone()));
            let current = l.books.get_mut(id).ok_or("Audiobook removed")?;
            if let Some((player, range)) = &playing {
                record_playback(&root, current, range, &player.snapshot())?;
            }
            if let Some(live) = &current.full_scan {
                scan.preserve_listening(live);
            }
            // Publish with the bookmark and source spans of the range the user actually heard.
            let mut publication = if let Some((_, range)) = &playing {
                current
                    .ranges()
                    .find(|b| &b.range_id == range)
                    .cloned()
                    .ok_or("Saved listening range unavailable")?
            } else if scan.draft.bookmark_sentence.is_some() {
                (*scan.draft).clone()
            } else {
                current.clone()
            };
            scan.publish(&root, &mut publication)?;
            *current = publication;
            let path = audio_path(&root, current)?;
            let samples = current.committed_samples;
            let position = current.position;
            let range_id = current.range_id.clone();
            if let Some((player, _)) = playing {
                if let Some(p) = l.playing.as_mut() {
                    p.range_id = range_id;
                }
                if let Err(error) = player
                    .update_file(&path, samples, true)
                    .and_then(|_| player.seek(position))
                {
                    player.stop();
                    l.playing = None;
                    return Err(error);
                }
            }
            let current = l.books.get(id).ok_or("Audiobook removed")?;
            refresh_chapter_files(&root, current)?;
            return Ok(());
        }
        return Err("The complete transcript is required before generating audio".into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audiobooks::tests::{book, reader};
    pub(super) fn fixture() -> (PathBuf, Book) {
        let root = std::env::temp_dir().join(format!("texthalo-full-gap-{}", identity()));
        fs::create_dir_all(&root).unwrap();
        let mut original = book();
        capture(&mut original,&reader(1,"saved1"),"Chapter One\nThe patient archivist opened the old wooden chest and carefully examined the map inside.").unwrap();
        recover(&root, &original).unwrap();
        while original.generated < original.sentences.len() {
            commit(&root, &mut original, &[0.25; 12]).unwrap();
        }
        capture(&mut original,&reader(3,"saved3"),"Chapter Three\nThe expedition returned to the village where all their friends had gathered to welcome them.").unwrap();
        while original.generated < original.sentences.len() {
            commit(&root, &mut original, &[0.75; 16]).unwrap();
        }
        original.end_verified = true;
        let bookmark = original.sentences.last().unwrap();
        original.position = (bookmark.start_sample + 4) as f64 / RATE as f64;
        remember_position(&mut original);
        save(&root, &original).unwrap();
        (root, original)
    }
    fn raw_page(page: &Page) -> String {
        if let Some(heading) = &page.heading {
            format!(
                "{}\n{}",
                heading,
                page.text.strip_prefix(heading).unwrap_or(&page.text).trim()
            )
        } else {
            page.text.clone()
        }
    }
    pub(super) fn scan_view(page: u64, end: bool) -> Value {
        let mut r = reader(page, &format!("scan{page}"));
        r["startOfBook"] = json!(page == 1);
        r["endOfBook"] = json!(end);
        r
    }
    #[test]
    fn progress_uses_one_transcript_orders_opening_pages_and_excludes_unfinished_audio() {
        let (root, mut original) = fixture();
        let mut scan = FullScan::new(&original);
        scan.recover(&root).unwrap();
        let mut opening = scan_view(1, false);
        opening["progress"] = json!("Location 1 of 1583 ● 0%");
        scan.capture(
            &opening,
            "The opening dedication introduces this book and thanks everyone who helped.",
            false,
        )
        .unwrap();
        scan.capture(
            &scan_view(8, false),
            "PROLOGUE\nThe patient archivist found a map hidden beneath the heavy wooden chest.",
            true,
        )
        .unwrap();
        scan.capture(&scan_view(11, true), "Chapter One\nThe travelers left the village early in the morning. They reached the mountain trail just before sunset.", true).unwrap();
        let boundary = scan.draft.checkpoints[2].sentence;
        let folder = scan.folder(&root).unwrap();
        while scan.draft.generated <= boundary {
            commit(&folder, &mut scan.draft, &[0.4; 24000]).unwrap();
        }
        original.full_scan = Some(Box::new(scan.clone()));
        let view = original.view();
        let chapters = view["checkpoints"].as_array().unwrap();
        assert_eq!(
            chapters.len(),
            3,
            "Old recordings must not inflate the full transcript"
        );
        assert_eq!(chapters[0]["label"], "Opening pages");
        assert_eq!(chapters[1]["label"], "PROLOGUE");
        assert_eq!(chapters[2]["label"], "Chapter One");
        assert_eq!(view["totalPages"], 3);
        assert_eq!(view["totalGenerated"], scan.draft.generated);
        assert_eq!(view["fullPreparation"]["completedChapters"], 2);
        let ready = chapters
            .iter()
            .filter(|c| c["complete"] == true)
            .map(|c| c["duration"].as_f64().unwrap())
            .sum::<f64>();
        assert_eq!(view["totalSeconds"].as_f64().unwrap(), ready);
        assert!(view["generatedSeconds"].as_f64().unwrap() > ready);
        let (_, previous_end, _) = saved_playback(&scan.draft, Some(0.)).unwrap();
        while scan.draft.generated < scan.draft.sentences.len() {
            commit(&folder, &mut scan.draft, &[0.4; 24000]).unwrap();
        }
        let (_, extended_end, _) = saved_playback(&scan.draft, Some(0.)).unwrap();
        assert!(
            extended_end > previous_end,
            "The playlist grows when another chapter finishes"
        );
        assert_eq!(extended_end, scan.draft.committed_samples);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn completed_draft_chapters_are_playable_before_publication_and_keep_live_bookmarks() {
        let (root, mut original) = fixture();
        let original_range = original.range_id.clone();
        let original_bytes = fs::read(audio_path(&root, &original).unwrap()).unwrap();
        let mut scan = FullScan::new(&original);
        scan.recover(&root).unwrap();
        scan.capture(&scan_view(1, false), &raw_page(&original.pages[0]), false)
            .unwrap();
        scan.capture(&scan_view(2, true), "Chapter Two\nThe travelers reached the mountain village and rested beside the warm fire.", true).unwrap();
        let boundary = scan.draft.checkpoints[1].sentence;
        let folder = scan.folder(&root).unwrap();
        while scan.draft.generated < boundary {
            commit(&folder, &mut scan.draft, &[0.4; 24000]).unwrap();
        }
        let range_id = scan.draft.range_id.clone();
        original.full_scan = Some(Box::new(scan.clone()));
        let view = original.view();
        let ready = view["checkpoints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["rangeId"] == range_id && c["complete"] == true)
            .unwrap();
        assert_eq!(ready["label"], "Chapter One");
        assert!(!original.complete);
        let (start, end, _) =
            saved_playback(scan.draft(), Some(ready["seconds"].as_f64().unwrap())).unwrap();
        assert_eq!(start, 0.);
        assert_eq!(end, scan.draft.committed_samples);
        assert!(saved_playback(scan.draft(), Some(end as f64 / RATE as f64 + 0.1)).is_none());
        let service = Audiobooks::default();
        {
            let mut library = service.0.lock().unwrap();
            library.books.insert(original.id.clone(), original.clone());
            library.producing = Some((original.id.clone(), "test-producer".into(), true));
            record_playback(
                &root,
                library.books.get_mut(&original.id).unwrap(),
                &range_id,
                &crate::pcm::PlaybackStatus {
                    position: 0.5,
                    speed: 1.5,
                    ..Default::default()
                },
            )
            .unwrap();
        }
        // The producer's snapshot predates playback. Its next commit must keep the new bookmark.
        commit(&folder, &mut scan.draft, &[0.5; 24000]).unwrap();
        persist(&service, &original.id, "test-producer", &root, &scan).unwrap();
        let mut live = service.0.lock().unwrap().books[&original.id].clone();
        assert_eq!(live.range_id, original_range);
        assert_eq!(live.full_scan.as_ref().unwrap().draft.position, 0.5);
        assert_eq!(live.full_scan.as_ref().unwrap().draft.speed, 1.5);
        assert!(service.0.lock().unwrap().producing.is_some());
        assert_eq!(
            fs::read(audio_path(&root, &original).unwrap()).unwrap(),
            original_bytes
        );
        recover_preparation(&root, &mut live);
        assert_eq!(live.full_scan.as_ref().unwrap().draft.position, 0.5);
        scan.preserve_listening(live.full_scan.as_ref().unwrap());
        let mut publication = live.clone();
        publication.bookmark_sentence = scan.draft.bookmark_sentence.clone();
        publication.bookmark_offset = scan.draft.bookmark_offset;
        publication.speed = scan.draft.speed;
        scan.publish(&root, &mut publication).unwrap();
        assert_eq!(publication.position, 0.5);
        assert_eq!(publication.speed, 1.5);
        assert!(publication.complete);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn full_scan_fills_a_missing_chapter_reuses_pcm_and_preserves_the_bookmark() {
        let (root, mut original) = fixture();
        let source_bytes = fs::read(audio_path(&root, &original).unwrap()).unwrap();
        let bookmark = original.bookmark_sentence.clone();
        let offset = original.bookmark_offset;
        assert_eq!(
            chapter_view(&original, 0, &original.checkpoints[0])["complete"],
            false
        );
        let mut scan = FullScan::new(&original);
        scan.recover(&root).unwrap();
        scan.capture(&scan_view(1, false), &raw_page(&original.pages[0]), false)
            .unwrap();
        while scan.draft.generated < scan.draft.sentences.len() {
            assert!(!scan.reuse(&root).unwrap());
            commit(&scan.folder(&root).unwrap(), &mut scan.draft, &[0.6; 12]).unwrap();
        }
        scan.capture(&scan_view(2,false),"Chapter Two\nThe missing chapter describes how the travelers crossed the mountain pass during the winter storm.",true).unwrap();
        while scan.draft.generated < scan.draft.sentences.len() {
            assert!(!scan.reuse(&root).unwrap());
            let folder = scan.folder(&root).unwrap();
            commit(&folder, &mut scan.draft, &[0.5; 14]).unwrap();
        }
        assert!(
            scan.replacement(&original).is_err(),
            "Must not publish before reaching the real book end"
        );
        scan.capture(&scan_view(3, true), &raw_page(&original.pages[1]), true)
            .unwrap();
        while scan.draft.generated < scan.draft.sentences.len() {
            assert!(scan.reuse(&root).unwrap());
        }
        assert_eq!(scan.reused, 1);
        let mut scan =
            serde_json::from_slice::<FullScan>(&serde_json::to_vec(&scan).unwrap()).unwrap();
        scan.recover(&root).unwrap();
        scan.publish(&root, &mut original).unwrap();
        assert!(original.complete && original.continuous);
        assert_eq!(
            original
                .checkpoints
                .iter()
                .map(|c| c.label.as_str())
                .collect::<Vec<_>>(),
            vec!["Chapter One", "Chapter Two", "Chapter Three"]
        );
        assert_eq!(original.bookmark_sentence, bookmark);
        assert_eq!(original.bookmark_offset, offset);
        assert!(stable_position(&original).unwrap() > 0.);
        let repaired = fs::read(audio_path(&root, &original).unwrap()).unwrap();
        assert_ne!(
            &repaired[44..68],
            &source_bytes[44..68],
            "Partial chapter must be regenerated"
        );
        assert_eq!(
            &repaired[repaired.len() - 32..],
            &source_bytes[source_bytes.len() - 32..],
            "Later chapter PCM must remain byte-identical"
        );
        assert_eq!(
            fs::read(root.join("audio.wav")).unwrap(),
            source_bytes,
            "Original recording must survive replacement"
        );
        for (i, c) in original.checkpoints.iter().enumerate() {
            assert_eq!(chapter_view(&original, i, c)["complete"], true);
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn full_scan_resumes_the_sentence_commit_after_interrupted_metadata_publication() {
        let (root, original) = fixture();
        let source = fs::read(audio_path(&root, &original).unwrap()).unwrap();
        let mut scan = FullScan::new(&original);
        scan.recover(&root).unwrap();
        let stale = serde_json::to_vec(&scan).unwrap();
        scan.capture(&scan_view(1, false), &raw_page(&original.pages[0]), false)
            .unwrap();
        assert!(!scan.reuse(&root).unwrap());
        let folder = scan.folder(&root).unwrap();
        commit(&folder, &mut scan.draft, &[0.5; 12]).unwrap();
        let mut wav = OpenOptions::new()
            .append(true)
            .open(audio_path(&folder, &scan.draft).unwrap())
            .unwrap();
        wav.write_all(&[99; 32]).unwrap();
        drop(wav);
        let mut resumed: FullScan = serde_json::from_slice(&stale).unwrap();
        resumed.recover(&root).unwrap();
        assert_eq!(resumed.draft.generated, scan.draft.generated);
        assert_eq!(resumed.reused, scan.reused);
        assert_eq!(
            fs::metadata(audio_path(&folder, &resumed.draft).unwrap())
                .unwrap()
                .len(),
            44 + resumed.draft.committed_samples * 2
        );
        assert_eq!(
            fs::read(audio_path(&root, &original).unwrap()).unwrap(),
            source
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn full_scan_refuses_unverified_turns_and_uncertain_cache_matches() {
        let (root, original) = fixture();
        let mut scan = FullScan::new(&original);
        scan.recover(&root).unwrap();
        assert!(scan
            .capture(&scan_view(2, false), "Chapter Two\nUnverified text.", false)
            .is_err());
        scan.capture(&scan_view(1, false), &raw_page(&original.pages[0]), false)
            .unwrap();
        let mut duplicate = scan.sources[0].clone();
        for s in &mut duplicate.sentences {
            s.id = identity();
        }
        scan.sources.push(duplicate);
        scan.index();
        assert!(
            !scan.reuse(&root).unwrap(),
            "Ambiguous passages must be generated rather than guessed"
        );
        scan.sources.truncate(1);
        scan.sources[0].voice.speed += 0.1;
        scan.index();
        assert!(
            !scan.reuse(&root).unwrap(),
            "Audio from a different narration speed cannot be mixed"
        );
        assert!(scan
            .capture(&scan_view(3, true), &raw_page(&original.pages[1]), false)
            .is_err());
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::tests::{fixture, scan_view};
    use super::*;
    use crate::audiobooks::tests::{book, reader};
    #[test]
    fn capture_finishes_and_saves_the_transcript_before_any_audio_generation() {
        let root = std::env::temp_dir().join(format!("texthalo-transcript-{}", identity()));
        fs::create_dir_all(&root).unwrap();
        let mut scan = FullScan::new(&book());
        scan.recover(&root).unwrap();
        let at = std::cell::Cell::new(1u64);
        let view = || {
            let mut r = scan_view(at.get(), at.get() == 3);
            r["navigationVerified"] = json!(true);
            r
        };
        let command = |request: Value| -> Result<Value, String> {
            match request["type"].as_str().unwrap() {
                "inspect" => Ok(view()),
                "navigate" => {
                    assert_eq!(request["direction"], "next");
                    at.set(at.get() + 1);
                    Ok(view())
                }
                "ocr" => Ok(
                    json!({"reader":view(),"text":format!("CHAPTER {}\nThe verified passage on this page is number {} and contains the complete chapter narration.",at.get(),at.get())}),
                ),
                other => panic!("Unexpected capture command: {other}"),
            }
        };
        prepare_transcript_with(
            &mut scan,
            &root,
            command,
            || true,
            |scan| {
                assert_eq!(scan.draft.generated, 0);
                assert_eq!(scan.draft.committed_samples, 0);
                save(&scan.folder(&root)?, &scan.draft)
            },
        )
        .unwrap();
        assert!(scan.transcript_ready());
        assert_eq!(scan.phase, "rendering");
        let transcript: Value = serde_json::from_slice(
            &fs::read(scan.folder(&root).unwrap().join("transcript.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(transcript["pages"].as_array().unwrap().len(), 3);
        assert_eq!(transcript["chapters"].as_array().unwrap().len(), 3);
        assert!(transcript["text"].as_str().unwrap().contains("number 3"));
        // Reopening a captured transcript never inspects or navigates the browser.
        let mut reopened: FullScan =
            serde_json::from_value(serde_json::to_value(&scan).unwrap()).unwrap();
        reopened.recover(&root).unwrap();
        prepare_transcript_with(
            &mut reopened,
            &root,
            |_| panic!("Chrome must not be required after capture"),
            || true,
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(reopened.draft.generated, 0);
        let mut offline = book();
        offline.full_scan = Some(Box::new(reopened));
        assert_eq!(
            key(&captured_reader(&offline, true).unwrap()).unwrap(),
            offline.key
        );
        assert!(captured_reader(&offline, false).is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn interrupted_capture_preserves_pages_without_claiming_a_complete_transcript() {
        let root = std::env::temp_dir().join(format!("texthalo-transcript-stop-{}", identity()));
        fs::create_dir_all(&root).unwrap();
        let mut scan = FullScan::new(&book());
        scan.recover(&root).unwrap();
        let active = std::cell::Cell::new(true);
        let command = |request: Value| -> Result<Value, String> {
            let reader = scan_view(1, false);
            match request["type"].as_str().unwrap() {
                "inspect" => Ok(reader),
                "ocr" => Ok(
                    json!({"reader":reader,"text":"CHAPTER ONE\nThis first passage is captured safely before the reader session is interrupted."}),
                ),
                _ => panic!("Cancellation must prevent any more page turns"),
            }
        };
        let result = prepare_transcript_with(
            &mut scan,
            &root,
            command,
            || active.get(),
            |scan| {
                save(&scan.folder(&root)?, &scan.draft)?;
                active.set(false);
                Ok(())
            },
        );
        assert_eq!(result.unwrap_err(), "Preparation stopped");
        assert!(!scan.transcript_ready());
        assert!(!scan.folder(&root).unwrap().join("transcript.json").exists());
        scan.recover(&root).unwrap();
        assert_eq!(scan.draft.pages.len(), 1);
        assert_eq!(scan.draft.generated, 0);
        assert_eq!(scan.phase, "scanning");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cache_identity_stays_with_the_original_occurrence_in_a_repeated_view() {
        let root = std::env::temp_dir().join(format!("texthalo-full-repeat-{}", identity()));
        fs::create_dir_all(&root).unwrap();
        let passage="The patient archivist opened the old wooden chest and carefully examined the map inside.";
        let raw = format!("{passage} {passage}");
        let mut original = book();
        original.from_beginning = false;
        capture_from(&mut original, &reader(1, "saved1"), &raw, passage.len() + 1).unwrap();
        recover(&root, &original).unwrap();
        commit(&root, &mut original, &[0.25; 12]).unwrap();
        let saved = original.sentences[0].id.clone();
        let mut scan = FullScan::new(&original);
        scan.recover(&root).unwrap();
        let mut r = reader(1, "scan1");
        r["startOfBook"] = json!(true);
        r["endOfBook"] = json!(true);
        scan.capture(&r, &raw, false).unwrap();
        assert!(
            !scan.reuse(&root).unwrap(),
            "First occurrence must not steal the saved second occurrence's bookmark"
        );
        let folder = scan.folder(&root).unwrap();
        commit(&folder, &mut scan.draft, &[0.5; 12]).unwrap();
        assert!(
            !scan.reuse(&root).unwrap(),
            "A partial imported chapter must be regenerated even at the right occurrence"
        );
        assert_ne!(scan.draft.sentences[0].id, saved);
        assert_ne!(scan.draft.sentences[1].id, saved);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn confirmed_image_only_cover_is_retained_without_synthesizing_text() {
        let mut original = book();
        let mut scan = FullScan::new(&original);
        let mut cover = reader(1, "cover");
        cover["progress"] = json!("Location 1 of 1583");
        cover["startOfBook"] = json!(true);
        scan.capture(&cover, "", false).unwrap();
        assert_eq!(scan.draft.pages.len(), 1);
        assert!(scan.draft.sentences.is_empty());
        let mut end = reader(2, "end");
        end["endOfBook"] = json!(true);
        scan.capture(
            &end,
            "The entire narrative on this short final page has been captured successfully.",
            true,
        )
        .unwrap();
        let root = std::env::temp_dir().join(format!("texthalo-full-image-{}", identity()));
        fs::create_dir_all(&root).unwrap();
        scan.recover(&root).unwrap();
        while scan.draft.generated < scan.draft.sentences.len() {
            let folder = scan.folder(&root).unwrap();
            commit(&folder, &mut scan.draft, &[0.5; 12]).unwrap();
        }
        scan.publish(&root, &mut original).unwrap();
        assert!(original.complete);
        assert!(original.pages[0].text.is_empty());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn zero_text_requires_two_stable_ocr_passes_and_never_swallows_capture_failures() {
        let calls = std::cell::Cell::new(0);
        let r = reader(1, "cover");
        let request = json!({"type":"ocr","bookId":r["bookId"],"fingerprint":r["fingerprint"]});
        let result = read_view_with(
            |request| {
                if request["type"] == "ocr" {
                    calls.set(calls.get() + 1);
                    Err("No readable text found in the page crop".into())
                } else {
                    Ok(r.clone())
                }
            },
            request.clone(),
        )
        .unwrap();
        assert_eq!(calls.get(), 2);
        assert_eq!(result["emptyConfirmed"], true);
        assert!(read_view_with(|_| Err("CHROME_MINIMIZED".into()), request.clone()).is_err());
        assert!(read_view_with(
            |request| if request["type"] == "ocr" {
                Err("No readable text found in the page crop".into())
            } else {
                Ok(reader(2, "moved"))
            },
            request
        )
        .is_err());
    }
    #[test]
    fn broken_shadow_progress_does_not_hide_or_truncate_the_original_recording() {
        let root = std::env::temp_dir().join(format!("texthalo-full-shadow-{}", identity()));
        fs::create_dir_all(&root).unwrap();
        let mut original = book();
        capture(&mut original,&reader(1,"saved1"),"The original recording must remain available after interrupted full audiobook preparation.").unwrap();
        recover(&root, &original).unwrap();
        commit(&root, &mut original, &[0.25; 12]).unwrap();
        let original_bytes = fs::read(audio_path(&root, &original).unwrap()).unwrap();
        let mut scan = FullScan::new(&original);
        scan.recover(&root).unwrap();
        fs::write(
            scan.folder(&root).unwrap().join("book.json"),
            b"not valid progress",
        )
        .unwrap();
        original.full_scan = Some(Box::new(scan));
        recover_preparation(&root, &mut original);
        assert!(original.full_scan.is_none());
        assert!(original
            .message
            .as_ref()
            .unwrap()
            .contains("original recording is available"));
        recover(&root, &original).unwrap();
        assert_eq!(
            fs::read(audio_path(&root, &original).unwrap()).unwrap(),
            original_bytes
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn resume_searches_all_views_sharing_the_saved_page_number() {
        let text = "The patient archivist opened the old wooden chest and carefully examined the map inside.";
        let mut b = book();
        capture(&mut b, &reader(10, "second-ten"), text).unwrap();
        let at = std::cell::Cell::new(0usize);
        let views = [
            reader(9, "nine"),
            reader(10, "first-ten"),
            reader(10, "second-ten"),
        ];
        let result = restore_scan_view_with(
            |request| match request["type"].as_str().unwrap() {
                "inspect" => Ok(views[at.get()].clone()),
                "navigate" => {
                    assert_eq!(request["direction"], "next");
                    at.set(at.get() + 1);
                    Ok(views[at.get()].clone())
                }
                "ocr" => Ok(
                    json!({"reader":views[at.get()], "text":if at.get() == 2 { text } else { "An entirely different passage about the lighthouse and the distant harbor." }}),
                ),
                _ => unreachable!(),
            },
            || true,
            &b,
            &b.pages[0],
        );
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(at.get(), 2);
    }
    #[test]
    fn publication_maps_an_imported_fragment_to_its_complete_sentence() {
        let (root, _) = fixture();
        let mut original = book();
        let full = "The travelers crossed the mountain pass during the winter storm and arrived home safely.";
        let fragment = "during the winter storm and arrived home safely.";
        capture_from(
            &mut original,
            &reader(1, "saved"),
            full,
            full.find(fragment).unwrap(),
        )
        .unwrap();
        recover(&root, &original).unwrap();
        while original.generated < original.sentences.len() {
            commit(&root, &mut original, &[0.25; 12]).unwrap();
        }
        original.bookmark_sentence = Some(original.sentences[0].id.clone());
        let mut scan = FullScan::new(&original);
        scan.recover(&root).unwrap();
        scan.capture(&scan_view(1, true), full, false).unwrap();
        while scan.draft.generated < scan.draft.sentences.len() {
            commit(&scan.folder(&root).unwrap(), &mut scan.draft, &[0.5; 24]).unwrap();
        }
        let next = scan.replacement(&original).unwrap();
        assert_eq!(next.bookmark_sentence, Some(next.sentences[0].id.clone()));
        assert_eq!(next.bookmark_offset, 0);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn resume_rejects_changed_view_extent_even_when_the_saved_prefix_matches() {
        let words = (0..220).map(|n| format!("word{n}")).collect::<Vec<_>>();
        let saved = words[..160].join(" ");
        let mut b = book();
        capture(&mut b, &reader(10, "saved-layout"), &saved).unwrap();
        for text in [words.join(" "), words[..120].join(" ")] {
            let mut changed = reader(10, "changed-layout");
            changed["endOfBook"] = json!(true);
            let result = restore_scan_view_with(
                |request| match request["type"].as_str().unwrap() {
                    "inspect" => Ok(changed.clone()),
                    "ocr" => Ok(json!({"reader":changed, "text":text})),
                    "navigate" => Err("Changed view was not accepted".into()),
                    _ => unreachable!(),
                },
                || true,
                &b,
                &b.pages[0],
            );
            assert!(
                result.is_err(),
                "Changed pagination must never be accepted by prefix/tail alone"
            );
        }
    }
    #[test]
    fn full_scan_rebuilds_partial_chapters_and_keeps_complete_chapters_cached() {
        let (root, mut original) = fixture();
        original.end_verified = true;
        let scan = FullScan::new(&original);
        assert!(!scan
            .cache
            .contains_key(&canonical(&original.sentences[0].text)));
        assert!(scan
            .cache
            .contains_key(&canonical(&original.sentences.last().unwrap().text)));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn identical_text_on_distinct_forward_pages_is_not_a_repeated_view() {
        let mut scan = FullScan::new(&book());
        let text = "The same printed passage appears again here on a separate verified page.";
        scan.capture(&scan_view(1, false), text, false).unwrap();
        scan.capture(&scan_view(2, true), text, true).unwrap();
        assert_eq!(scan.draft.pages.len(), 2);
        assert_eq!(scan.draft.sentences.len(), 2);
        assert!(scan.capture(&scan_view(2, true), text, true).is_err());
    }
    #[test]
    fn stale_pixels_are_recaptured_without_another_page_turn() {
        let mut b = book();
        let old =
            "The first page contains this complete sentence with enough words to recognize it.";
        capture(&mut b, &reader(1, "old"), old).unwrap();
        let next = reader(2, "next");
        let reads = std::cell::Cell::new(0);
        let result = read_fresh_view_with(|request| match request["type"].as_str().unwrap() {
            "ocr" => {
                reads.set(reads.get() + 1);
                Ok(json!({"reader":next, "text":if reads.get() < 3 { old } else { "The second page finally rendered and its new passage is ready for capture." }}))
            }
            "inspect" => Ok(next.clone()),
            _ => panic!("A suspect capture must not turn another page"),
        }, || true, &b, next.clone(), true).unwrap();
        assert_eq!(reads.get(), 3);
        assert!(result["text"]
            .as_str()
            .unwrap()
            .starts_with("The second page"));
    }
    #[test]
    fn legacy_preparation_using_partial_audio_restarts_without_changing_original_audio() {
        let (root, original) = fixture();
        let bytes = fs::read(audio_path(&root, &original).unwrap()).unwrap();
        let mut scan = FullScan::new(&original);
        scan.recover(&root).unwrap();
        scan.capture(&scan_view(1, false), &original.pages[0].text, false)
            .unwrap();
        scan.draft.sentences[0].id = original.sentences[0].id.clone();
        commit(&scan.folder(&root).unwrap(), &mut scan.draft, &[0.5; 12]).unwrap();
        scan.reuse_policy = 0;
        scan.recover(&root).unwrap();
        assert!(scan.draft.pages.is_empty());
        assert_eq!(scan.reuse_policy, 1);
        assert_eq!(
            fs::read(audio_path(&root, &original).unwrap()).unwrap(),
            bytes
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn persistent_old_pixels_never_commit_as_a_new_page_despite_forward_metadata() {
        let mut b = book();
        let old = "This is the old rendered passage still showing while the next page is loading.";
        capture(&mut b, &reader(1, "old"), old).unwrap();
        let next = reader(2, "next");
        let result = read_fresh_view_with(
            |request| match request["type"].as_str().unwrap() {
                "ocr" => Ok(json!({"reader":next,"text":old})),
                "inspect" => Ok(next.clone()),
                _ => panic!("Must not navigate past an unverified screenshot"),
            },
            || true,
            &b,
            next.clone(),
            true,
        );
        assert!(result.is_err());
        assert_eq!(b.pages.len(), 1);
    }
}
