pub mod structural;
pub mod tags;

/// The structural index — flat arrays, no DOM tree.
///
/// Built from XML bytes in one pass (scalar or SIMD). Enables random-access
/// evaluation of all 13 XPath 1.0 axes via array operations instead of
/// pointer-chasing through a DOM.
///
/// Memory: ~16 bytes per tag vs ~35 bytes per node in a typical DOM.
pub struct XmlIndex<'a> {
    /// Original XML bytes (borrowed, not copied)
    pub(crate) input: &'a [u8],

    /// Byte offset of each '<' (start of each tag/comment/PI)
    pub(crate) tag_starts: Vec<u32>,

    /// Byte offset of each '>' (end of each tag/comment/PI)
    pub(crate) tag_ends: Vec<u32>,

    /// Tag type classification
    pub tag_types: Vec<TagType>,

    /// Tag name: (byte offset, length) into input
    pub(crate) tag_names: Vec<(u32, u16)>,

    /// Nesting depth of each tag (0 = root level)
    pub(crate) depths: Vec<u16>,

    /// Index of parent tag (into tag_starts array). Root tags have parent = u32::MAX.
    pub(crate) parents: Vec<u32>,

    /// Text content ranges: (start_offset, end_offset) for text between tags
    pub(crate) text_ranges: Vec<TextRange>,

    // === Precomputed indices (built by `build_indices()`) ===

    /// CSR children: offsets[i]..offsets[i+1] into child_data gives children of tag i.
    pub(crate) child_offsets: Vec<u32>,
    /// Flat array of child tag indices, referenced by child_offsets.
    pub(crate) child_data: Vec<u32>,

    /// CSR text children: text_offsets[i]..text_offsets[i+1] into text_data.
    pub(crate) text_child_offsets: Vec<u32>,
    /// Flat array of text range indices, referenced by text_child_offsets.
    pub(crate) text_child_data: Vec<u32>,

    /// Matching close tag for each open tag. u32::MAX = no match.
    pub(crate) close_map: Vec<u32>,

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagType {
    Open,      // <tag>
    Close,     // </tag>
    SelfClose, // <tag/>
    Comment,   // <!-- ... -->
    CData,     // <![CDATA[ ... ]]>
    PI,        // <?target ... ?>
}

/// A text content node between tags.
#[derive(Debug, Clone, Copy)]
pub struct TextRange {
    /// Byte offset of text start
    pub start: u32,
    /// Byte offset of text end (exclusive)
    pub end: u32,
    /// Index of the parent open tag
    pub parent_tag: u32,
}

impl<'a> XmlIndex<'a> {
    /// Build precomputed indices for fast XPath evaluation.
    /// Called once after structural parsing. O(n) time, flat memory layout.
    pub(crate) fn build_indices(&mut self) {
        let n = self.tag_count();

        // 1. Count children per parent (two-pass CSR build)
        let mut child_counts = vec![0u32; n + 1];
        for i in 0..n {
            let tt = self.tag_types[i];
            if tt == TagType::Close || tt == TagType::CData {
                continue;
            }
            let parent = self.parents[i];
            if parent != u32::MAX && (parent as usize) < n {
                child_counts[parent as usize] += 1;
            }
        }

        // Prefix sum → offsets
        let mut child_offsets = vec![0u32; n + 1];
        for i in 0..n {
            child_offsets[i + 1] = child_offsets[i] + child_counts[i];
        }
        let total_children = child_offsets[n] as usize;
        let mut child_data = vec![0u32; total_children];

        // Fill child_data (second pass)
        let mut write_pos = child_offsets.clone();
        for i in 0..n {
            let tt = self.tag_types[i];
            if tt == TagType::Close || tt == TagType::CData {
                continue;
            }
            let parent = self.parents[i];
            if parent != u32::MAX && (parent as usize) < n {
                let p = parent as usize;
                child_data[write_pos[p] as usize] = i as u32;
                write_pos[p] += 1;
            }
        }

        // 2. CSR for text children
        let mut text_counts = vec![0u32; n + 1];
        for range in &self.text_ranges {
            let parent = range.parent_tag;
            if parent != u32::MAX && (parent as usize) < n {
                text_counts[parent as usize] += 1;
            }
        }
        let mut text_child_offsets = vec![0u32; n + 1];
        for i in 0..n {
            text_child_offsets[i + 1] = text_child_offsets[i] + text_counts[i];
        }
        let total_text = text_child_offsets[n] as usize;
        let mut text_child_data = vec![0u32; total_text];
        let mut text_write_pos = text_child_offsets.clone();
        for (ti, range) in self.text_ranges.iter().enumerate() {
            let parent = range.parent_tag;
            if parent != u32::MAX && (parent as usize) < n {
                let p = parent as usize;
                text_child_data[text_write_pos[p] as usize] = ti as u32;
                text_write_pos[p] += 1;
            }
        }

        // 3. Close map using a stack (O(n))
        let mut close_map = vec![u32::MAX; n];
        let mut stack: Vec<usize> = Vec::new();
        for i in 0..n {
            match self.tag_types[i] {
                TagType::Open => stack.push(i),
                TagType::Close => {
                    if let Some(open_idx) = stack.pop() {
                        close_map[open_idx] = i as u32;
                    }
                }
                TagType::SelfClose => close_map[i] = i as u32,
                _ => {}
            }
        }

        self.child_offsets = child_offsets;
        self.child_data = child_data;
        self.text_child_offsets = text_child_offsets;
        self.text_child_data = text_child_data;
        self.close_map = close_map;
    }

    /// Get child tag indices for a parent (from precomputed CSR index).
    #[inline]
    pub(crate) fn child_tag_slice(&self, parent_idx: usize) -> &[u32] {
        if parent_idx >= self.child_offsets.len() - 1 {
            return &[];
        }
        let start = self.child_offsets[parent_idx] as usize;
        let end = self.child_offsets[parent_idx + 1] as usize;
        &self.child_data[start..end]
    }

    /// Get child text range indices for a parent (from precomputed CSR index).
    #[inline]
    pub(crate) fn child_text_slice(&self, parent_idx: usize) -> &[u32] {
        if parent_idx >= self.text_child_offsets.len() - 1 {
            return &[];
        }
        let start = self.text_child_offsets[parent_idx] as usize;
        let end = self.text_child_offsets[parent_idx + 1] as usize;
        &self.text_child_data[start..end]
    }

    /// Fast tag name comparison (avoids UTF-8 validation on the hot path).
    #[inline]
    pub fn tag_name_eq(&self, tag_idx: usize, name: &str) -> bool {
        let (off, len) = self.tag_names[tag_idx];
        let name_bytes = name.as_bytes();
        if name_bytes.len() != len as usize { return false; }
        &self.input[off as usize..off as usize + len as usize] == name_bytes
    }

    /// Get the tag name as a string slice.
    pub fn tag_name(&self, tag_idx: usize) -> &'a str {
        if tag_idx >= self.tag_names.len() {
            return "";
        }
        let (offset, len) = self.tag_names[tag_idx];
        let bytes = &self.input[offset as usize..(offset + len as u32) as usize];
        std::str::from_utf8(bytes).unwrap_or("")
    }

    /// Get the text content of a text range.
    pub fn text_content(&self, range: &TextRange) -> &'a str {
        let bytes = &self.input[range.start as usize..range.end as usize];
        std::str::from_utf8(bytes).unwrap_or("")
    }

    /// Number of tags in the index.
    pub fn tag_count(&self) -> usize {
        self.tag_starts.len()
    }

    /// Number of text content ranges.
    pub fn text_count(&self) -> usize {
        self.text_ranges.len()
    }

    /// Find the index of the close tag matching an open tag.
    pub fn matching_close(&self, open_idx: usize) -> Option<usize> {
        if open_idx >= self.tag_count() {
            return None;
        }
        // Use precomputed close_map if available
        if !self.close_map.is_empty() {
            let close = self.close_map[open_idx];
            return if close != u32::MAX { Some(close as usize) } else { None };
        }
        // Fallback: linear scan (used before build_indices)
        if self.tag_types[open_idx] == TagType::SelfClose {
            return Some(open_idx);
        }
        if self.tag_types[open_idx] != TagType::Open {
            return None;
        }
        let depth = self.depths[open_idx];
        let name = self.tag_name(open_idx);
        for i in (open_idx + 1)..self.tag_count() {
            if self.tag_types[i] == TagType::Close
                && self.depths[i] == depth
                && self.tag_name(i) == name
            {
                return Some(i);
            }
        }
        None
    }

    /// Get children (direct child open/self-close tags) of a tag.
    pub fn children(&self, parent_idx: usize) -> Vec<usize> {
        self.child_tag_slice(parent_idx).iter().map(|&i| i as usize).collect()
    }

    /// Get text content directly under a tag (not nested).
    pub fn direct_text(&self, tag_idx: usize) -> Vec<&'a str> {
        self.child_text_slice(tag_idx).iter()
            .map(|&ti| self.text_content(&self.text_ranges[ti as usize]))
            .collect()
    }

    /// Get all text content under a tag (including nested).
    pub fn all_text(&self, tag_idx: usize) -> String {
        let close_idx = self.matching_close(tag_idx).unwrap_or(tag_idx);
        let start = self.tag_ends[tag_idx] as usize + 1;
        let end = self.tag_starts[close_idx] as usize;
        if start >= end || start >= self.input.len() {
            return String::new();
        }
        // Strip all tags, keep only text
        let mut result = String::new();
        let slice = &self.input[start..end.min(self.input.len())];
        let mut in_tag = false;
        for &b in slice {
            if b == b'<' {
                in_tag = true;
            } else if b == b'>' {
                in_tag = false;
            } else if !in_tag {
                result.push(b as char);
            }
        }
        result
    }
}
