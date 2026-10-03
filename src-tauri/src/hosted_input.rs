//! Ordered upload state for one hosted incremental speech session.
#[derive(Default)]
pub struct Upload {
    pub text: String,
    pub finished: bool,
    sequence: u64,
}
impl Upload {
    pub fn append(&mut self, text: &str) -> Result<u64, String> {
        if self.finished || text.is_empty() { return Err("Invalid speech input sequence.".into()); }
        if self.text.chars().count() + text.chars().count() > 20_000 { return Err("Speech exceeds 20,000 characters.".into()); }
        let sequence = self.sequence;
        self.sequence += 1;
        self.text.push_str(text);
        Ok(sequence)
    }
    pub fn finish(&mut self) -> Result<u64, String> {
        if self.finished || self.text.trim().is_empty() { return Err("Speech ended without valid text.".into()); }
        self.finished = true;
        Ok(self.sequence)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordered_delta_upload_has_one_explicit_eof() {
        let mut upload = Upload::default();
        assert_eq!(upload.append("First. ").unwrap(), 0);
        assert_eq!(upload.append("Second.").unwrap(), 1);
        assert_eq!(upload.finish().unwrap(), 2);
        assert_eq!(upload.text, "First. Second.");
        assert!(upload.finish().is_err());
        assert!(upload.append("Third.").is_err());
    }
    #[test]
    fn empty_and_oversized_inputs_never_finish_successfully() {
        let mut upload = Upload::default();
        assert!(upload.finish().is_err());
        assert!(upload.append("").is_err());
        assert!(upload.append(&"😀".repeat(20001)).is_err());
        assert_eq!(upload.text, "");
        assert_eq!(upload.append(&"😀".repeat(20000)).unwrap(), 0);
        assert!(upload.append("!").is_err());
        assert_eq!(upload.text.chars().count(), 20000);
    }
}
