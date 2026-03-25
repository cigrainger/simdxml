use thiserror::Error;

#[derive(Debug, Error)]
pub enum SimdXmlError {
    #[error("XML parse error at byte {offset}: {message}")]
    ParseError { offset: usize, message: String },

    #[error("XPath parse error: {0}")]
    XPathParseError(String),

    #[error("XPath evaluation error: {0}")]
    XPathEvalError(String),

    #[error("Unclosed tag at byte {0}")]
    UnclosedTag(usize),

    #[error("Mismatched close tag: expected </{expected}>, got </{found}> at byte {offset}")]
    MismatchedCloseTag {
        expected: String,
        found: String,
        offset: usize,
    },

    #[error("Invalid XML: {0}")]
    InvalidXml(String),
}

pub type Result<T> = std::result::Result<T, SimdXmlError>;
