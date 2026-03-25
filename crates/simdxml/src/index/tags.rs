// Tag-level utilities — attribute parsing, namespace resolution.
// Will be expanded in later phases.

use crate::index::XmlIndex;

impl<'a> XmlIndex<'a> {
    /// Extract an attribute value from a tag by attribute name.
    pub fn get_attribute(&self, tag_idx: usize, attr_name: &str) -> Option<&'a str> {
        let start = self.tag_starts[tag_idx] as usize;
        let end = self.tag_ends[tag_idx] as usize;
        let tag_bytes = &self.input[start..=end];

        // Simple attribute parsing: find attr_name="value" or attr_name='value'
        let attr_search = format!("{}=", attr_name);
        let tag_str = std::str::from_utf8(tag_bytes).ok()?;

        let attr_pos = tag_str.find(&attr_search)?;
        let after_eq = attr_pos + attr_search.len();
        let rest = &tag_str[after_eq..];

        let (quote_char, rest) = if rest.starts_with('"') {
            ('"', &rest[1..])
        } else if rest.starts_with('\'') {
            ('\'', &rest[1..])
        } else {
            return None;
        };

        let end_quote = rest.find(quote_char)?;
        // Return a slice from the original input
        let abs_start = start + after_eq + 1;
        let abs_end = abs_start + end_quote;
        std::str::from_utf8(&self.input[abs_start..abs_end]).ok()
    }
}

#[cfg(test)]
mod tests {
    use crate::index::structural::parse_scalar;

    #[test]
    fn test_get_attribute() {
        let xml = b"<root lang=\"en\" type='main'>text</root>";
        let index = parse_scalar(xml).unwrap();
        assert_eq!(index.get_attribute(0, "lang"), Some("en"));
        assert_eq!(index.get_attribute(0, "type"), Some("main"));
        assert_eq!(index.get_attribute(0, "missing"), None);
    }

    #[test]
    fn test_attribute_on_self_closing() {
        let xml = b"<br class=\"clear\"/>";
        let index = parse_scalar(xml).unwrap();
        assert_eq!(index.get_attribute(0, "class"), Some("clear"));
    }
}
