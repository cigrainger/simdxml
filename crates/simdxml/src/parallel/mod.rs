//! Speculative parallel chunked parsing.
//!
//! Split a large XML document into K chunks at safe boundaries (between tags),
//! parse each chunk in parallel to extract structural positions, then merge
//! and compute depth/parent relationships in a single sequential pass.
//!
//! The parallel work (memchr scanning + tag classification) is the expensive
//! part of parsing. The sequential merge (depth/parent computation) is O(n)
//! and very fast since it's just a stack-based walk over tag types.
//!
//! # Algorithm
//!
//! 1. Find K-1 safe split points (positions of `>` between tags)
//! 2. Spawn K threads, each parsing its chunk independently
//! 3. Each thread produces: tag_starts, tag_ends, tag_types, tag_names, text_ranges
//! 4. Merge chunks (concatenate — already in document order)
//! 5. Compute depths and parents in one sequential pass
//! 6. Build CSR indices

use crate::error::{Result, SimdXmlError};
use crate::index::{TagType, TextRange, XmlIndex};
use memchr::memchr;

/// Minimum document size (bytes) to benefit from parallel parsing.
/// Below this, the thread overhead exceeds the parallel speedup.
const MIN_PARALLEL_SIZE: usize = 64 * 1024; // 64 KB

/// Parse XML using multiple threads.
///
/// Falls back to sequential parsing for small documents or when `num_threads <= 1`.
pub fn parse_parallel<'a>(input: &'a [u8], num_threads: usize) -> Result<XmlIndex<'a>> {
    if num_threads <= 1 || input.len() < MIN_PARALLEL_SIZE {
        return crate::index::structural::parse_scalar(input);
    }

    let num_threads = num_threads.min(input.len() / (MIN_PARALLEL_SIZE / 2));

    // Find safe split points
    let splits = find_split_points(input, num_threads);
    let num_chunks = splits.len() + 1;

    // Parse chunks in parallel
    let chunk_results: Vec<ChunkResult> = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(num_chunks);

        for i in 0..num_chunks {
            let start = if i == 0 { 0 } else { splits[i - 1] };
            let end = if i < splits.len() { splits[i] } else { input.len() };
            let chunk = &input[start..end];
            let chunk_start = start;

            handles.push(scope.spawn(move || {
                parse_chunk(input, chunk, chunk_start)
            }));
        }

        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    // Merge chunk results
    merge_chunks(input, chunk_results)
}

/// A single chunk's parse results (no depth/parent — those need global state).
struct ChunkResult {
    tag_starts: Vec<u32>,
    tag_ends: Vec<u32>,
    tag_types: Vec<TagType>,
    tag_names: Vec<(u32, u16)>,
    text_ranges: Vec<TextRange>,
}

/// Find K-1 safe split points in the input.
///
/// A safe split point is a position just after a `>` that's between tags
/// (not inside a comment, CDATA, or quoted attribute). We scan backward
/// from each desired split position to find the nearest `>`.
fn find_split_points(input: &[u8], num_chunks: usize) -> Vec<usize> {
    let chunk_size = input.len() / num_chunks;
    let mut splits = Vec::with_capacity(num_chunks - 1);

    for i in 1..num_chunks {
        let target = i * chunk_size;
        if let Some(pos) = find_safe_boundary(input, target) {
            // Don't create empty chunks or duplicate splits
            let last = splits.last().copied().unwrap_or(0);
            if pos > last && pos < input.len() {
                splits.push(pos);
            }
        }
    }

    splits
}

/// Find a safe boundary near `target` — a position just after `>` that's between tags.
fn find_safe_boundary(input: &[u8], target: usize) -> Option<usize> {
    let target = target.min(input.len());

    // Search backward from target for a `>` not inside a comment/CDATA
    let search_start = target.saturating_sub(4096); // don't search too far back
    for pos in (search_start..target).rev() {
        if input[pos] == b'>' {
            // Check this isn't inside a comment (-->) or CDATA (]]>)
            // by verifying the next non-whitespace char is `<` or EOF
            let after = pos + 1;
            if after >= input.len() {
                return Some(after);
            }

            // Skip whitespace after >
            let mut check = after;
            while check < input.len() && input[check].is_ascii_whitespace() {
                check += 1;
            }

            // Next meaningful char should be < (start of next tag) or EOF
            if check >= input.len() || input[check] == b'<' {
                return Some(after);
            }
            // Otherwise this > is inside text content — keep searching
        }
    }

    // Fallback: search forward from target
    for pos in target..input.len() {
        if input[pos] == b'>' {
            return Some(pos + 1);
        }
    }

    None
}

/// Parse a single chunk, extracting structural positions.
///
/// `full_input` is the complete XML (for tag name references).
/// `chunk` is the slice being parsed.
/// `chunk_start` is the byte offset of `chunk` within `full_input`.
fn parse_chunk<'a>(
    _full_input: &'a [u8],
    chunk: &'a [u8],
    chunk_start: usize,
) -> ChunkResult {
    let est_tags = chunk.len() / 128;
    let est_text = est_tags / 2;

    let mut result = ChunkResult {
        tag_starts: Vec::with_capacity(est_tags),
        tag_ends: Vec::with_capacity(est_tags),
        tag_types: Vec::with_capacity(est_tags),
        tag_names: Vec::with_capacity(est_tags),
        text_ranges: Vec::with_capacity(est_text),
    };

    let mut pos = 0;
    let mut last_tag_end: usize = 0;

    // We use a simple open-tag counter for text range parent tracking within this chunk.
    // The real parent will be computed during merge. We store u32::MAX as placeholder.
    while let Some(offset) = memchr(b'<', &chunk[pos..]) {
        pos += offset;
        let abs_pos = chunk_start + pos;
        let tag_start = pos;

        // Text content between previous tag end and this tag start
        {
            let text_start = if last_tag_end > 0 { last_tag_end + 1 } else { 0 };
            if text_start < tag_start {
                result.text_ranges.push(TextRange {
                    start: (chunk_start + text_start) as u32,
                    end: abs_pos as u32,
                    parent_tag: u32::MAX, // placeholder — resolved during merge
                });
            }
        }

        if pos + 1 >= chunk.len() {
            break;
        }

        match chunk[pos + 1] {
            b'/' => {
                // Close tag
                pos += 2;
                let name_start = pos;
                while pos < chunk.len() && chunk[pos] != b'>' && !chunk[pos].is_ascii_whitespace() {
                    pos += 1;
                }
                let name_end = pos;

                if let Some(off) = memchr(b'>', &chunk[pos..]) {
                    pos += off;
                } else {
                    break;
                }

                result.tag_starts.push(abs_pos as u32);
                result.tag_ends.push((chunk_start + pos) as u32);
                result.tag_types.push(TagType::Close);
                result.tag_names.push(((chunk_start + name_start) as u32, (name_end - name_start) as u16));

                last_tag_end = pos;
                pos += 1;
            }
            b'!' => {
                if chunk.get(pos + 2..pos + 4) == Some(b"--") {
                    // Comment
                    result.tag_starts.push(abs_pos as u32);
                    result.tag_types.push(TagType::Comment);
                    result.tag_names.push((0, 0));

                    pos += 4;
                    loop {
                        if let Some(off) = memchr(b'-', &chunk[pos..]) {
                            pos += off;
                            if pos + 2 < chunk.len() && &chunk[pos..pos + 3] == b"-->" {
                                pos += 2;
                                break;
                            }
                            pos += 1;
                        } else {
                            pos = chunk.len();
                            break;
                        }
                    }
                    result.tag_ends.push((chunk_start + pos) as u32);
                    last_tag_end = pos;
                    pos += 1;
                } else if chunk.get(pos + 2..pos + 9) == Some(b"[CDATA[") {
                    // CDATA
                    result.tag_starts.push(abs_pos as u32);
                    result.tag_types.push(TagType::CData);
                    result.tag_names.push((0, 0));

                    pos += 9;
                    let content_start = pos;
                    loop {
                        if let Some(off) = memchr(b']', &chunk[pos..]) {
                            pos += off;
                            if pos + 2 < chunk.len() && &chunk[pos..pos + 3] == b"]]>" {
                                if pos > content_start {
                                    result.text_ranges.push(TextRange {
                                        start: (chunk_start + content_start) as u32,
                                        end: (chunk_start + pos) as u32,
                                        parent_tag: u32::MAX,
                                    });
                                }
                                pos += 2;
                                break;
                            }
                            pos += 1;
                        } else {
                            break;
                        }
                    }
                    result.tag_ends.push((chunk_start + pos) as u32);
                    last_tag_end = pos;
                    pos += 1;
                } else {
                    // DOCTYPE or other — skip
                    if let Some(off) = memchr(b'>', &chunk[pos..]) {
                        pos += off;
                    }
                    last_tag_end = pos;
                    pos += 1;
                }
            }
            b'?' => {
                // Processing instruction
                pos += 2;
                let name_start = pos;
                while pos < chunk.len()
                    && chunk[pos] != b'?'
                    && chunk[pos] != b'>'
                    && !chunk[pos].is_ascii_whitespace()
                {
                    pos += 1;
                }
                let name_end = pos;

                result.tag_starts.push(abs_pos as u32);
                result.tag_types.push(TagType::PI);
                result.tag_names.push(((chunk_start + name_start) as u32, (name_end - name_start) as u16));

                while pos + 1 < chunk.len() {
                    if chunk[pos] == b'?' && chunk[pos + 1] == b'>' {
                        pos += 1;
                        break;
                    }
                    pos += 1;
                }
                result.tag_ends.push((chunk_start + pos) as u32);
                last_tag_end = pos;
                pos += 1;
            }
            _ => {
                // Open or self-closing tag
                pos += 1;
                let name_start = pos;
                while pos < chunk.len()
                    && chunk[pos] != b'>'
                    && chunk[pos] != b'/'
                    && !chunk[pos].is_ascii_whitespace()
                {
                    pos += 1;
                }
                let name_end = pos;

                let mut self_closing = false;
                while pos < chunk.len() && chunk[pos] != b'>' {
                    if chunk[pos] == b'/' && pos + 1 < chunk.len() && chunk[pos + 1] == b'>' {
                        self_closing = true;
                        pos += 1;
                        break;
                    }
                    if chunk[pos] == b'"' {
                        pos += 1;
                        if let Some(off) = memchr(b'"', &chunk[pos..]) { pos += off; }
                    } else if chunk[pos] == b'\'' {
                        pos += 1;
                        if let Some(off) = memchr(b'\'', &chunk[pos..]) { pos += off; }
                    }
                    pos += 1;
                }

                if pos >= chunk.len() {
                    break;
                }

                let tag_type = if self_closing { TagType::SelfClose } else { TagType::Open };

                result.tag_starts.push(abs_pos as u32);
                result.tag_ends.push((chunk_start + pos) as u32);
                result.tag_types.push(tag_type);
                result.tag_names.push(((chunk_start + name_start) as u32, (name_end - name_start) as u16));

                last_tag_end = pos;
                pos += 1;
            }
        }
    }

    result
}

/// Merge chunk results into a single XmlIndex.
///
/// Concatenates structural arrays (already in document order), then computes
/// depth and parent relationships in a single sequential pass.
fn merge_chunks<'a>(input: &'a [u8], chunks: Vec<ChunkResult>) -> Result<XmlIndex<'a>> {
    // Count totals for pre-allocation
    let total_tags: usize = chunks.iter().map(|c| c.tag_starts.len()).sum();
    let total_text: usize = chunks.iter().map(|c| c.text_ranges.len()).sum();

    let mut tag_starts = Vec::with_capacity(total_tags);
    let mut tag_ends = Vec::with_capacity(total_tags);
    let mut tag_types = Vec::with_capacity(total_tags);
    let mut tag_names = Vec::with_capacity(total_tags);
    let mut text_ranges = Vec::with_capacity(total_text);

    for chunk in chunks {
        tag_starts.extend_from_slice(&chunk.tag_starts);
        tag_ends.extend_from_slice(&chunk.tag_ends);
        tag_types.extend_from_slice(&chunk.tag_types);
        tag_names.extend_from_slice(&chunk.tag_names);
        text_ranges.extend_from_slice(&chunk.text_ranges);
    }

    // === Compute depth, parents, AND fix text range parents in one pass ===
    // Text ranges are sorted by start position (same order as tags).
    // We walk tags and text ranges together, using the parent stack to assign
    // text range parents in O(n + t) total.
    let n = tag_types.len();
    let mut depths = Vec::with_capacity(n);
    let mut parents = Vec::with_capacity(n);
    let mut depth: u16 = 0;
    let mut parent_stack: Vec<u32> = Vec::new();

    let mut text_idx = 0; // cursor into text_ranges

    for i in 0..n {
        let tag_pos = tag_starts[i];

        // Assign parents to any text ranges that come before this tag
        while text_idx < text_ranges.len() && text_ranges[text_idx].start < tag_pos {
            text_ranges[text_idx].parent_tag = parent_stack.last().copied().unwrap_or(u32::MAX);
            text_idx += 1;
        }

        match tag_types[i] {
            TagType::Close => {
                if depth > 0 { depth -= 1; }
                parent_stack.pop();
                depths.push(depth);
                parents.push(parent_stack.last().copied().unwrap_or(u32::MAX));
            }
            TagType::Open => {
                depths.push(depth);
                parents.push(parent_stack.last().copied().unwrap_or(u32::MAX));
                parent_stack.push(i as u32);
                depth += 1;
            }
            TagType::SelfClose | TagType::Comment | TagType::CData | TagType::PI => {
                depths.push(depth);
                parents.push(parent_stack.last().copied().unwrap_or(u32::MAX));
            }
        }
    }

    // Handle any remaining text ranges after the last tag
    while text_idx < text_ranges.len() {
        text_ranges[text_idx].parent_tag = parent_stack.last().copied().unwrap_or(u32::MAX);
        text_idx += 1;
    }

    let mut index = XmlIndex {
        input,
        tag_starts,
        tag_ends,
        tag_types,
        tag_names,
        depths,
        parents,
        text_ranges,
        child_offsets: Vec::new(),
        child_data: Vec::new(),
        text_child_offsets: Vec::new(),
        text_child_data: Vec::new(),
        close_map: Vec::new(),
        post_order: Vec::new(),
        name_ids: Vec::new(),
        name_table: Vec::new(),
        name_posting: Vec::new(),
    };

    if index.tag_count() >= 64 {
        index.build_indices();
    }

    Ok(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parallel_matches_sequential_small() {
        let xml = b"<root><a>1</a><b>2</b><c>3</c></root>";
        let seq = crate::parse(xml).unwrap();
        let par = parse_parallel(xml, 2).unwrap(); // falls back to sequential (too small)

        assert_eq!(seq.tag_count(), par.tag_count());
        assert_eq!(seq.tag_types, par.tag_types);
    }

    #[test]
    fn find_safe_boundary_basic() {
        let xml = b"<root><a>text</a><b>more</b></root>";
        let boundary = find_safe_boundary(xml, 15).unwrap();
        // Should find a > followed by <
        assert!(boundary > 0 && boundary <= xml.len());
        assert!(xml[boundary - 1] == b'>');
    }

    #[test]
    fn split_points_reasonable() {
        // Create a ~128KB document
        let mut xml = String::from("<root>");
        for i in 0..2000 {
            xml.push_str(&format!("<item id=\"{}\">content {}</item>", i, i));
        }
        xml.push_str("</root>");
        let bytes = xml.as_bytes();

        let splits = find_split_points(bytes, 4);
        // Should have up to 3 split points for 4 chunks
        assert!(!splits.is_empty());
        assert!(splits.len() <= 3);

        // Each split should be at a > boundary
        for &s in &splits {
            assert!(s > 0);
            assert_eq!(bytes[s - 1], b'>');
        }
    }

    #[test]
    fn parallel_parse_large_doc() {
        // Build a document large enough to trigger parallel parsing
        let mut xml = String::from("<corpus>");
        for i in 0..2000 {
            xml.push_str(&format!(
                "<patent id=\"{}\"><title>Patent {}</title><claims><claim>Claim text {}</claim></claims></patent>",
                i, i, i
            ));
        }
        xml.push_str("</corpus>");
        let bytes = xml.as_bytes();
        assert!(bytes.len() > MIN_PARALLEL_SIZE);

        let seq = crate::parse(bytes).unwrap();
        let par = parse_parallel(bytes, 4).unwrap();

        // Same number of tags
        assert_eq!(seq.tag_count(), par.tag_count(),
            "tag count: seq={} par={}", seq.tag_count(), par.tag_count());

        // Same tag types
        assert_eq!(seq.tag_types, par.tag_types);

        // Same tag positions
        assert_eq!(seq.tag_starts, par.tag_starts);
        assert_eq!(seq.tag_ends, par.tag_ends);

        // Same depths
        assert_eq!(seq.depths, par.depths);

        // Same parents
        assert_eq!(seq.parents, par.parents);
    }

    #[test]
    fn parallel_xpath_equivalence() {
        let mut xml = String::from("<corpus>");
        for i in 0..2000 {
            xml.push_str(&format!(
                "<patent><title>Title {}</title><claim>Claim {}</claim></patent>",
                i, i
            ));
        }
        xml.push_str("</corpus>");
        let bytes = xml.as_bytes();

        let seq = crate::parse(bytes).unwrap();
        let par = parse_parallel(bytes, 4).unwrap();

        let queries = ["//title", "//claim", "//patent", "/corpus/patent/title"];
        for q in &queries {
            let seq_results = seq.xpath_text(q).unwrap();
            let par_results = par.xpath_text(q).unwrap();
            assert_eq!(seq_results.len(), par_results.len(),
                "count mismatch for {}: seq={} par={}", q, seq_results.len(), par_results.len());
            assert_eq!(seq_results, par_results, "text mismatch for {}", q);
        }
    }

    #[test]
    fn parallel_thread_counts() {
        let mut xml = String::from("<r>");
        for i in 0..3000 {
            xml.push_str(&format!("<item>{}</item>", i));
        }
        xml.push_str("</r>");
        let bytes = xml.as_bytes();

        let seq = crate::parse(bytes).unwrap();

        for threads in [1, 2, 4, 8] {
            let par = parse_parallel(bytes, threads).unwrap();
            assert_eq!(seq.tag_count(), par.tag_count(),
                "tag count mismatch with {} threads", threads);
            assert_eq!(seq.tag_types, par.tag_types,
                "tag types mismatch with {} threads", threads);
        }
    }

    #[test]
    fn parallel_with_attributes() {
        let mut xml = String::from("<root>");
        for i in 0..2000 {
            xml.push_str(&format!(
                r#"<item id="{}" class="c{}" data-value="{}">content</item>"#,
                i, i % 10, i * 100
            ));
        }
        xml.push_str("</root>");
        let bytes = xml.as_bytes();

        let seq = crate::parse(bytes).unwrap();
        let par = parse_parallel(bytes, 4).unwrap();

        assert_eq!(seq.tag_count(), par.tag_count());
        assert_eq!(seq.tag_starts, par.tag_starts);

        // Attribute access should work
        let seq_text = seq.xpath_text("//item").unwrap();
        let par_text = par.xpath_text("//item").unwrap();
        assert_eq!(seq_text, par_text);
    }
}
