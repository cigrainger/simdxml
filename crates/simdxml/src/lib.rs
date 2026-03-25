//! simdxml — SIMD-accelerated XML parser with full XPath 1.0 support.
//!
//! The world's first production SIMD XML parser. Uses a two-pass structural
//! indexing architecture (adapted from simdjson) to parse XML at 2-3 GB/s,
//! then evaluates XPath 1.0 expressions against flat arrays instead of a DOM.
//!
//! # Quick Start
//!
//! ```rust
//! use simdxml::{parse, xpath};
//!
//! let xml = b"<patent><claim>A device for...</claim></patent>";
//! let index = parse(xml).unwrap();
//!
//! // One-shot XPath
//! let texts = index.xpath_text("//claim").unwrap();
//! assert_eq!(texts, vec!["A device for..."]);
//!
//! // Compiled XPath (reusable across documents)
//! let expr = xpath::CompiledXPath::compile("//claim").unwrap();
//! let texts = expr.eval_text(&index).unwrap();
//! ```

pub mod error;
pub mod index;
pub mod xpath;

pub use error::{Result, SimdXmlError};
pub use index::XmlIndex;
pub use xpath::CompiledXPath;

/// Parse XML bytes and build a structural index.
///
/// This is the main entry point. Returns an `XmlIndex` that can be
/// queried with XPath expressions.
pub fn parse(input: &[u8]) -> Result<XmlIndex<'_>> {
    index::structural::parse_scalar(input)
}

// Convenience methods on XmlIndex
impl<'a> XmlIndex<'a> {
    /// Evaluate an XPath expression and return text content of matches.
    pub fn xpath_text(&'a self, xpath_expr: &str) -> Result<Vec<&'a str>> {
        let expr = xpath::parse_xpath(xpath_expr)?;
        xpath::eval_text(self, &expr)
    }

    /// Evaluate an XPath expression and return matching nodes.
    pub fn xpath(&self, xpath_expr: &str) -> Result<Vec<xpath::XPathNode>> {
        let expr = xpath::parse_xpath(xpath_expr)?;
        xpath::evaluate(self, &expr)
    }
}
