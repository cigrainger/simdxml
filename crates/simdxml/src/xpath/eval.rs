use crate::error::{Result, SimdXmlError};
use crate::index::{TagType, XmlIndex};
use super::ast::*;

/// A node in the XPath result set — either a tag index or a text range index.
#[derive(Debug, Clone, Copy)]
pub enum XPathNode {
    Element(usize),   // index into tag_starts
    Text(usize),      // index into text_ranges
    Attribute(usize, usize), // (tag_idx, attr_name offset) — placeholder
}

/// Evaluate an XPath expression against an XmlIndex.
pub fn evaluate<'a>(
    index: &'a XmlIndex<'a>,
    expr: &XPathExpr,
) -> Result<Vec<XPathNode>> {
    match expr {
        XPathExpr::LocationPath(path) => eval_location_path(index, path),
        XPathExpr::Union(exprs) => {
            let mut result = Vec::new();
            for e in exprs {
                result.extend(evaluate(index, e)?);
            }
            Ok(result)
        }
        _ => Err(SimdXmlError::XPathEvalError(
            "Only location paths and unions are supported".into(),
        )),
    }
}

/// Extract text results from XPath evaluation.
pub fn eval_text<'a>(
    index: &'a XmlIndex<'a>,
    expr: &XPathExpr,
) -> Result<Vec<&'a str>> {
    let nodes = evaluate(index, expr)?;
    let mut results = Vec::new();
    for node in nodes {
        match node {
            XPathNode::Element(idx) => {
                // For elements, return all text content
                let text = index.all_text(idx);
                if !text.is_empty() {
                    // We need to return owned strings for all_text, but &str for direct text
                    // For now, use direct text
                    for t in index.direct_text(idx) {
                        results.push(t);
                    }
                }
            }
            XPathNode::Text(idx) => {
                results.push(index.text_content(&index.text_ranges[idx]));
            }
            XPathNode::Attribute(tag_idx, _) => {
                // Placeholder — attribute value extraction
            }
        }
    }
    Ok(results)
}

/// Sentinel index for the virtual document root.
const DOC_ROOT: usize = usize::MAX;

fn eval_location_path<'a>(
    index: &'a XmlIndex<'a>,
    path: &LocationPath,
) -> Result<Vec<XPathNode>> {
    let mut context: Vec<XPathNode> = if path.absolute {
        // Virtual document root — child axis returns depth-0 elements
        vec![XPathNode::Element(DOC_ROOT)]
    } else {
        return Err(SimdXmlError::XPathEvalError(
            "Relative paths require a context node".into(),
        ));
    };

    for step in &path.steps {
        context = eval_step(index, &context, step)?;
    }

    Ok(context)
}

fn eval_step<'a>(
    index: &'a XmlIndex<'a>,
    context: &[XPathNode],
    step: &Step,
) -> Result<Vec<XPathNode>> {
    let mut result = Vec::new();

    for &node in context {
        let candidates = match step.axis {
            Axis::Child => eval_child_axis(index, node),
            Axis::Descendant => eval_descendant_axis(index, node, false),
            Axis::DescendantOrSelf => eval_descendant_axis(index, node, true),
            Axis::Parent => eval_parent_axis(index, node),
            Axis::Ancestor => eval_ancestor_axis(index, node, false),
            Axis::AncestorOrSelf => eval_ancestor_axis(index, node, true),
            Axis::FollowingSibling => eval_following_sibling_axis(index, node),
            Axis::PrecedingSibling => eval_preceding_sibling_axis(index, node),
            Axis::Following => eval_following_axis(index, node),
            Axis::Preceding => eval_preceding_axis(index, node),
            Axis::SelfAxis => vec![node],
            Axis::Attribute => eval_attribute_axis(index, node, &step.node_test),
            Axis::Namespace => vec![], // TODO
        };

        for candidate in candidates {
            if matches_node_test(index, candidate, &step.node_test) {
                result.push(candidate);
            }
        }
    }

    Ok(result)
}

fn matches_node_test(index: &XmlIndex, node: XPathNode, test: &NodeTest) -> bool {
    match (node, test) {
        (_, NodeTest::Node) => true,
        (_, NodeTest::Wildcard) => matches!(node, XPathNode::Element(_)),
        (XPathNode::Text(_), NodeTest::Text) => true,
        (XPathNode::Element(idx), NodeTest::Name(name)) => index.tag_name(idx) == name,
        (XPathNode::Element(idx), NodeTest::Comment) => {
            index.tag_types[idx] == TagType::Comment
        }
        (XPathNode::Element(idx), NodeTest::PI) => index.tag_types[idx] == TagType::PI,
        (XPathNode::Attribute(_, _), NodeTest::Name(_)) => true, // already matched in axis
        _ => false,
    }
}

// ============================================================================
// Axis implementations — all 13 axes as array operations on XmlIndex
// ============================================================================

fn eval_child_axis(index: &XmlIndex, node: XPathNode) -> Vec<XPathNode> {
    let XPathNode::Element(parent_idx) = node else {
        return vec![];
    };

    let mut result: Vec<XPathNode> = Vec::new();

    if parent_idx == DOC_ROOT {
        // Document root's children are depth-0 elements
        for i in 0..index.tag_count() {
            if index.depths[i] == 0
                && (index.tag_types[i] == TagType::Open
                    || index.tag_types[i] == TagType::SelfClose)
            {
                result.push(XPathNode::Element(i));
            }
        }
        return result;
    }

    // Child elements
    for i in 0..index.tag_count() {
        if index.parents[i] == parent_idx as u32
            && (index.tag_types[i] == TagType::Open
                || index.tag_types[i] == TagType::SelfClose)
        {
            result.push(XPathNode::Element(i));
        }
    }

    // Child text nodes
    for (i, range) in index.text_ranges.iter().enumerate() {
        if range.parent_tag == parent_idx as u32 {
            result.push(XPathNode::Text(i));
        }
    }

    result
}

fn eval_descendant_axis(index: &XmlIndex, node: XPathNode, include_self: bool) -> Vec<XPathNode> {
    let XPathNode::Element(start_idx) = node else {
        return if include_self { vec![node] } else { vec![] };
    };

    if start_idx == DOC_ROOT {
        // Descendants of document root = all elements + text nodes
        let mut result = Vec::new();
        for i in 0..index.tag_count() {
            if index.tag_types[i] == TagType::Open || index.tag_types[i] == TagType::SelfClose {
                result.push(XPathNode::Element(i));
            }
        }
        for i in 0..index.text_ranges.len() {
            result.push(XPathNode::Text(i));
        }
        return result;
    }

    let mut result = Vec::new();
    if include_self {
        result.push(node);
    }

    let _start_depth = index.depths[start_idx];

    // All tags after start_idx until depth returns to start_depth
    let close_idx = index.matching_close(start_idx).unwrap_or(index.tag_count());
    for i in (start_idx + 1)..close_idx {
        if index.tag_types[i] == TagType::Open || index.tag_types[i] == TagType::SelfClose {
            result.push(XPathNode::Element(i));
        }
    }

    // Descendant text nodes
    for (i, range) in index.text_ranges.iter().enumerate() {
        let parent = range.parent_tag as usize;
        if parent >= start_idx && parent < close_idx {
            result.push(XPathNode::Text(i));
        }
    }

    result
}

fn eval_parent_axis(index: &XmlIndex, node: XPathNode) -> Vec<XPathNode> {
    match node {
        XPathNode::Element(idx) => {
            let parent = index.parents[idx];
            if parent != u32::MAX {
                vec![XPathNode::Element(parent as usize)]
            } else {
                vec![]
            }
        }
        XPathNode::Text(idx) => {
            let parent = index.text_ranges[idx].parent_tag;
            if parent != u32::MAX {
                vec![XPathNode::Element(parent as usize)]
            } else {
                vec![]
            }
        }
        _ => vec![],
    }
}

fn eval_ancestor_axis(index: &XmlIndex, node: XPathNode, include_self: bool) -> Vec<XPathNode> {
    let mut result = Vec::new();
    if include_self {
        result.push(node);
    }

    let mut current = match node {
        XPathNode::Element(idx) => index.parents[idx],
        XPathNode::Text(idx) => index.text_ranges[idx].parent_tag,
        _ => u32::MAX,
    };

    while current != u32::MAX {
        result.push(XPathNode::Element(current as usize));
        current = index.parents[current as usize];
    }

    result
}

fn eval_following_sibling_axis(index: &XmlIndex, node: XPathNode) -> Vec<XPathNode> {
    let XPathNode::Element(idx) = node else {
        return vec![];
    };

    let parent = index.parents[idx];
    let depth = index.depths[idx];
    let mut result = Vec::new();

    for i in (idx + 1)..index.tag_count() {
        if index.parents[i] == parent
            && index.depths[i] == depth
            && (index.tag_types[i] == TagType::Open
                || index.tag_types[i] == TagType::SelfClose)
        {
            result.push(XPathNode::Element(i));
        }
    }

    result
}

fn eval_preceding_sibling_axis(index: &XmlIndex, node: XPathNode) -> Vec<XPathNode> {
    let XPathNode::Element(idx) = node else {
        return vec![];
    };

    let parent = index.parents[idx];
    let depth = index.depths[idx];
    let mut result = Vec::new();

    for i in (0..idx).rev() {
        if index.parents[i] == parent
            && index.depths[i] == depth
            && (index.tag_types[i] == TagType::Open
                || index.tag_types[i] == TagType::SelfClose)
        {
            result.push(XPathNode::Element(i));
        }
    }

    result
}

fn eval_following_axis(index: &XmlIndex, node: XPathNode) -> Vec<XPathNode> {
    let XPathNode::Element(idx) = node else {
        return vec![];
    };

    let close = index.matching_close(idx).unwrap_or(idx);
    let mut result = Vec::new();

    for i in (close + 1)..index.tag_count() {
        if index.tag_types[i] == TagType::Open || index.tag_types[i] == TagType::SelfClose {
            result.push(XPathNode::Element(i));
        }
    }

    result
}

fn eval_preceding_axis(index: &XmlIndex, node: XPathNode) -> Vec<XPathNode> {
    let XPathNode::Element(idx) = node else {
        return vec![];
    };

    let mut result = Vec::new();

    // All elements before this one, excluding ancestors
    let ancestors: Vec<u32> = {
        let mut a = Vec::new();
        let mut current = index.parents[idx];
        while current != u32::MAX {
            a.push(current);
            current = index.parents[current as usize];
        }
        a
    };

    for i in (0..idx).rev() {
        if (index.tag_types[i] == TagType::Open || index.tag_types[i] == TagType::SelfClose)
            && !ancestors.contains(&(i as u32))
        {
            result.push(XPathNode::Element(i));
        }
    }

    result
}

fn eval_attribute_axis(
    index: &XmlIndex,
    node: XPathNode,
    test: &NodeTest,
) -> Vec<XPathNode> {
    let XPathNode::Element(idx) = node else {
        return vec![];
    };

    match test {
        NodeTest::Name(name) => {
            if index.get_attribute(idx, name).is_some() {
                vec![XPathNode::Attribute(idx, 0)]
            } else {
                vec![]
            }
        }
        NodeTest::Wildcard => {
            // TODO: return all attributes
            vec![]
        }
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::structural::parse_scalar;
    use crate::xpath::parser::parse_xpath;

    fn query_text<'a>(xml: &'a [u8], xpath: &str) -> Vec<String> {
        let index = parse_scalar(xml).unwrap();
        let expr = parse_xpath(xpath).unwrap();
        let nodes = evaluate(&index, &expr).unwrap();
        let mut results = Vec::new();
        for node in nodes {
            match node {
                XPathNode::Element(idx) => {
                    for t in index.direct_text(idx) {
                        results.push(t.to_string());
                    }
                }
                XPathNode::Text(idx) => {
                    results.push(index.text_content(&index.text_ranges[idx]).to_string());
                }
                _ => {}
            }
        }
        results
    }

    fn query_names<'a>(xml: &'a [u8], xpath: &str) -> Vec<String> {
        let index = parse_scalar(xml).unwrap();
        let expr = parse_xpath(xpath).unwrap();
        let nodes = evaluate(&index, &expr).unwrap();
        nodes
            .iter()
            .filter_map(|n| match n {
                XPathNode::Element(idx) => Some(index.tag_name(*idx).to_string()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn test_simple_child() {
        let names = query_names(b"<root><a/><b/><c/></root>", "/root/*");
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_specific_child() {
        let names = query_names(b"<root><a/><b/><c/></root>", "/root/b");
        assert_eq!(names, vec!["b"]);
    }

    #[test]
    fn test_text_content() {
        let texts = query_text(b"<root><item>hello</item><item>world</item></root>", "/root/item");
        assert_eq!(texts, vec!["hello", "world"]);
    }

    #[test]
    fn test_descendant() {
        let names = query_names(
            b"<root><a><b><c/></b></a></root>",
            "//c",
        );
        assert_eq!(names, vec!["c"]);
    }

    #[test]
    fn test_text_node() {
        let texts = query_text(
            b"<root>hello</root>",
            "/root/text()",
        );
        assert_eq!(texts, vec!["hello"]);
    }

    #[test]
    fn test_wildcard() {
        let names = query_names(
            b"<root><a/><b/></root>",
            "/root/*",
        );
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn test_deep_path() {
        let texts = query_text(
            b"<patent><claims><claim>Claim 1 text</claim></claims></patent>",
            "/patent/claims/claim",
        );
        assert_eq!(texts, vec!["Claim 1 text"]);
    }

    #[test]
    fn test_descendant_deep() {
        let names = query_names(
            b"<a><b><c><d/></c></b><e><d/></e></a>",
            "//d",
        );
        assert_eq!(names.len(), 2);
    }

    #[test]
    fn test_following_sibling() {
        let xml = b"<root><a/><b/><c/></root>";
        let index = parse_scalar(xml).unwrap();
        // Get siblings following <a>
        let a_idx = 1; // <a> is at index 1 (after <root>)
        let siblings = eval_following_sibling_axis(&index, XPathNode::Element(a_idx));
        assert_eq!(siblings.len(), 2); // b and c
    }

    #[test]
    fn test_preceding_sibling() {
        let xml = b"<root><a/><b/><c/></root>";
        let index = parse_scalar(xml).unwrap();
        let c_idx = 3; // <c> is at index 3
        let siblings = eval_preceding_sibling_axis(&index, XPathNode::Element(c_idx));
        assert_eq!(siblings.len(), 2); // a and b (in reverse order)
    }

    #[test]
    fn test_parent() {
        let xml = b"<root><child/></root>";
        let index = parse_scalar(xml).unwrap();
        let parents = eval_parent_axis(&index, XPathNode::Element(1)); // child's parent
        assert_eq!(parents.len(), 1);
        match parents[0] {
            XPathNode::Element(idx) => assert_eq!(index.tag_name(idx), "root"),
            _ => panic!("Expected element"),
        }
    }

    #[test]
    fn test_ancestor() {
        let xml = b"<a><b><c/></b></a>";
        let index = parse_scalar(xml).unwrap();
        let ancestors = eval_ancestor_axis(&index, XPathNode::Element(2), false); // c's ancestors
        assert_eq!(ancestors.len(), 2); // b and a
    }
}
