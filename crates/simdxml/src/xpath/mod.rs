//! XPath 1.0 evaluation engine.
//!
//! Parses XPath expressions into an AST, then evaluates them against an
//! [`XmlIndex`] using array operations instead of pointer-chasing through a DOM.
//! All 13 XPath axes are supported, along with predicates, functions, and operators.

pub(crate) mod analyze;
pub mod ast;
pub(crate) mod eval;
pub(crate) mod parser;
pub(crate) mod simd_pred;

// Public types and functions
pub use ast::XPathExpr;
pub use eval::{eval_standalone_expr, extract_text, StandaloneResult, XPathNode, XPathResult};

// Internal functions used by lib.rs and CLI
// Used internally and by conformance tests
#[doc(hidden)]
pub use eval::{evaluate, evaluate_from_context};
pub(crate) use eval::{eval_text, eval_expr_with_doc, eval_expr_with_context, eval_xpath};
#[doc(hidden)]
pub use parser::parse_xpath;

use crate::error::Result;
use crate::index::XmlIndex;

/// Compiled XPath expression — reusable across documents.
///
/// Compile once, evaluate many times. Avoids re-parsing the expression string
/// on each call. Use this for batch processing or repeated queries.
///
/// ```rust
/// let compiled = simdxml::CompiledXPath::compile("//claim").unwrap();
/// let xml = b"<r><claim>A</claim><claim>B</claim></r>";
/// let index = simdxml::parse(xml).unwrap();
/// let results = compiled.eval_text(&index).unwrap();
/// assert_eq!(results, vec!["A", "B"]);
/// ```
pub struct CompiledXPath {
    expr: XPathExpr,
}

impl CompiledXPath {
    /// Compile an XPath expression.
    pub fn compile(xpath: &str) -> Result<Self> {
        let expr = parse_xpath(xpath)?;
        Ok(Self { expr })
    }

    /// Evaluate and return matching nodes.
    pub fn eval<'a>(&self, index: &'a XmlIndex<'a>) -> Result<Vec<XPathNode>> {
        evaluate(index, &self.expr)
    }

    /// Evaluate and return text content of matching nodes.
    pub fn eval_text<'a>(&self, index: &'a XmlIndex<'a>) -> Result<Vec<&'a str>> {
        eval_text(index, &self.expr)
    }

    /// Analyze this expression for query-driven lazy parsing.
    ///
    /// Returns the set of tag names referenced, or `None` if the query
    /// uses wildcards/node() and requires all tags.
    pub fn interesting_names(&self) -> Option<std::collections::HashSet<String>> {
        match analyze::selectivity(&self.expr) {
            analyze::SelectivityHint::Selective(names) => Some(names),
            analyze::SelectivityHint::NeedsAll => None,
        }
    }

    /// Access the underlying parsed expression.
    pub fn expr(&self) -> &XPathExpr {
        &self.expr
    }
}
