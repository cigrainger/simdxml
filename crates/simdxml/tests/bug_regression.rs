/// Regression tests for bugs found by the edge-case audit.

fn xpath_count(xml: &[u8], expr: &str) -> usize {
    let index = simdxml::parse(xml).unwrap();
    index.xpath(expr).unwrap().len()
}

fn xpath_text(xml: &[u8], expr: &str) -> Vec<String> {
    let index = simdxml::parse(xml).unwrap();
    index.xpath_text(expr).unwrap().iter().map(|s| s.to_string()).collect()
}

// Bug 1: [last()] should select the last node, not all nodes.
// last() returns a number (context size). In predicate context,
// a number N means position()=N. So [last()] = [position()=last()].
#[test]
fn test_last_as_predicate() {
    let xml = b"<r><a/><a/><a/></r>";
    assert_eq!(xpath_count(xml, "/r/a[last()]"), 1, "[last()] should select only the last node");
    assert_eq!(xpath_count(xml, "/r/a[1]"), 1);
    assert_eq!(xpath_count(xml, "/r/a[position()=last()]"), 1);
}

// Bug 2: @* should match attribute nodes (Wildcard should match Attribute variant)
#[test]
fn test_attribute_wildcard_matches() {
    let xml = b"<r x='1' y='2'/>";
    assert_eq!(xpath_count(xml, "/r/@*"), 2, "@* should return all attributes");
}

// Bug 3: not(@attr) should test node-set existence, not string truthiness.
// <a x=""/> has attribute x with empty value — not(@x) should be false.
#[test]
fn test_not_attribute_existence() {
    let xml = b"<r><a x=''/><a/></r>";
    // not(@x) should be true only for <a/> (which lacks @x), not for <a x=""/>
    assert_eq!(xpath_count(xml, "/r/a[not(@x)]"), 1, "not(@x) should test existence, not string value");
}

// Bug 4: preceding::* should exclude ancestors
#[test]
fn test_preceding_excludes_ancestors() {
    let xml = b"<r><a><b/></a><c/></r>";
    // preceding::* from b should be empty (a and r are ancestors, not preceding)
    assert_eq!(xpath_count(xml, "/r/a/b/preceding::*"), 0, "preceding should exclude ancestors");
}
