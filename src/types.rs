#[derive(Clone, Debug, PartialEq)]
pub struct ClipboardItem {
    pub content: ClipboardContent,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ClipboardContent {
    Text(String),
    Image(ImageData),
}

#[derive(Clone, Debug)]
pub struct ImageData {
    pub width: usize,
    pub height: usize,
    pub bytes: Vec<u8>,
}

// Manual implementation of PartialEq for ImageData to avoid huge comparisons if not needed,
// but for correctness we should compare bytes.
impl PartialEq for ImageData {
    fn eq(&self, other: &Self) -> bool {
        self.width == other.width && self.height == other.height && self.bytes == other.bytes
    }
}
