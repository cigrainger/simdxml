use nom::{
    branch::alt,
    bytes::complete::{tag, take_while1},
    character::complete::{char, multispace0},
    combinator::opt,
    multi::separated_list1,
    sequence::{delimited, pair, preceded, tuple},
    IResult,
};

use super::ast::*;
use crate::error::{Result, SimdXmlError};

/// Parse an XPath 1.0 expression string.
pub fn parse_xpath(input: &str) -> Result<XPathExpr> {
    let input = input.trim();
    match xpath_expr(input) {
        Ok(("", expr)) => Ok(expr),
        Ok((rest, _)) => Err(SimdXmlError::XPathParseError(format!(
            "Unexpected trailing input: '{rest}'"
        ))),
        Err(e) => Err(SimdXmlError::XPathParseError(format!("{e}"))),
    }
}

fn xpath_expr(input: &str) -> IResult<&str, XPathExpr> {
    // For now, parse location paths and simple expressions
    // Full XPath 1.0 expression grammar will be expanded
    alt((union_expr, location_path_expr))(input)
}

fn union_expr(input: &str) -> IResult<&str, XPathExpr> {
    let (input, first) = location_path_expr(input)?;
    let (input, rest) = nom::multi::many0(preceded(
        delimited(multispace0, char('|'), multispace0),
        location_path_expr,
    ))(input)?;

    if rest.is_empty() {
        Ok((input, first))
    } else {
        let mut all = vec![first];
        all.extend(rest);
        Ok((input, XPathExpr::Union(all)))
    }
}

fn location_path_expr(input: &str) -> IResult<&str, XPathExpr> {
    let (input, path) = location_path(input)?;
    Ok((input, XPathExpr::LocationPath(path)))
}

fn location_path(input: &str) -> IResult<&str, LocationPath> {
    alt((absolute_path, abbreviated_descendant_path, relative_path))(input)
}

/// Absolute path: /step/step/...
fn absolute_path(input: &str) -> IResult<&str, LocationPath> {
    let (input, _) = char('/')(input)?;

    // Check for // at root
    if input.starts_with('/') {
        let (input, _) = char('/')(input)?;
        let (input, mut steps) = separated_list1(char('/'), step)(input)?;
        // Prepend descendant-or-self::node() for //
        steps.insert(
            0,
            Step {
                axis: Axis::DescendantOrSelf,
                node_test: NodeTest::Node,
                predicates: vec![],
            },
        );
        Ok((
            input,
            LocationPath {
                absolute: true,
                steps,
            },
        ))
    } else if input.is_empty() || input.starts_with('|') || input.starts_with(')') {
        // Bare / — select root
        Ok((
            input,
            LocationPath {
                absolute: true,
                steps: vec![],
            },
        ))
    } else {
        let (input, steps) = separated_list1(
            alt((
                // Handle // within path
                nom::combinator::map(tag("//"), |_| true),
                nom::combinator::map(char('/'), |_| false),
            )),
            step,
        )(input)?;
        Ok((
            input,
            LocationPath {
                absolute: true,
                steps,
            },
        ))
    }
}

/// Abbreviated descendant: //step/step/...
fn abbreviated_descendant_path(input: &str) -> IResult<&str, LocationPath> {
    let (input, _) = tag("//")(input)?;
    let (input, steps) = separated_list1(char('/'), step)(input)?;
    let mut all_steps = vec![Step {
        axis: Axis::DescendantOrSelf,
        node_test: NodeTest::Node,
        predicates: vec![],
    }];
    all_steps.extend(steps);
    Ok((
        input,
        LocationPath {
            absolute: true,
            steps: all_steps,
        },
    ))
}

/// Relative path: step/step/...
fn relative_path(input: &str) -> IResult<&str, LocationPath> {
    let (input, steps) = separated_list1(char('/'), step)(input)?;
    Ok((
        input,
        LocationPath {
            absolute: false,
            steps,
        },
    ))
}

/// A single step: axis::nodetest[predicate]
fn step(input: &str) -> IResult<&str, Step> {
    let (input, _) = multispace0(input)?;

    // Check for abbreviated axes
    if input.starts_with("..") {
        let (input, _) = tag("..")(input)?;
        return Ok((
            input,
            Step {
                axis: Axis::Parent,
                node_test: NodeTest::Node,
                predicates: vec![],
            },
        ));
    }
    if input.starts_with('.') && !input[1..].starts_with('.') {
        let (input, _) = char('.')(input)?;
        return Ok((
            input,
            Step {
                axis: Axis::SelfAxis,
                node_test: NodeTest::Node,
                predicates: vec![],
            },
        ));
    }

    // Check for @attr (abbreviated attribute axis)
    if input.starts_with('@') {
        let (input, _) = char('@')(input)?;
        let (input, test) = node_test(input)?;
        let (input, preds) = predicates(input)?;
        return Ok((
            input,
            Step {
                axis: Axis::Attribute,
                node_test: test,
                predicates: preds,
            },
        ));
    }

    // Check for explicit axis:: syntax
    if let Ok((rest, axis)) = axis_specifier(input) {
        let (rest, test) = node_test(rest)?;
        let (rest, preds) = predicates(rest)?;
        return Ok((
            rest,
            Step {
                axis,
                node_test: test,
                predicates: preds,
            },
        ));
    }

    // Default: child axis
    let (input, test) = node_test(input)?;
    let (input, preds) = predicates(input)?;
    Ok((
        input,
        Step {
            axis: Axis::Child,
            node_test: test,
            predicates: preds,
        },
    ))
}

fn axis_specifier(input: &str) -> IResult<&str, Axis> {
    let (input, name) = take_while1(|c: char| c.is_alphanumeric() || c == '-')(input)?;
    let (input, _) = tag("::")(input)?;
    let axis = match name {
        "child" => Axis::Child,
        "descendant" => Axis::Descendant,
        "parent" => Axis::Parent,
        "ancestor" => Axis::Ancestor,
        "following-sibling" => Axis::FollowingSibling,
        "preceding-sibling" => Axis::PrecedingSibling,
        "following" => Axis::Following,
        "preceding" => Axis::Preceding,
        "self" => Axis::SelfAxis,
        "descendant-or-self" => Axis::DescendantOrSelf,
        "ancestor-or-self" => Axis::AncestorOrSelf,
        "attribute" => Axis::Attribute,
        "namespace" => Axis::Namespace,
        _ => {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Tag,
            )));
        }
    };
    Ok((input, axis))
}

fn node_test(input: &str) -> IResult<&str, NodeTest> {
    alt((
        node_type_test,
        wildcard_test,
        name_test,
    ))(input)
}

fn node_type_test(input: &str) -> IResult<&str, NodeTest> {
    let (input, name) = take_while1(|c: char| c.is_alphanumeric() || c == '-')(input)?;
    let (input, _) = tag("()")(input)?;
    let test = match name {
        "text" => NodeTest::Text,
        "node" => NodeTest::Node,
        "comment" => NodeTest::Comment,
        "processing-instruction" => NodeTest::PI,
        _ => {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Tag,
            )));
        }
    };
    Ok((input, test))
}

fn wildcard_test(input: &str) -> IResult<&str, NodeTest> {
    let (input, _) = char('*')(input)?;
    Ok((input, NodeTest::Wildcard))
}

fn name_test(input: &str) -> IResult<&str, NodeTest> {
    let (input, name) = take_while1(|c: char| c.is_alphanumeric() || c == '-' || c == '_' || c == ':')(input)?;
    if let Some(colon_pos) = name.find(':') {
        let prefix = &name[..colon_pos];
        let local = &name[colon_pos + 1..];
        Ok((
            input,
            NodeTest::NamespacedName(prefix.to_string(), local.to_string()),
        ))
    } else {
        Ok((input, NodeTest::Name(name.to_string())))
    }
}

fn predicates(input: &str) -> IResult<&str, Vec<XPathExpr>> {
    // TODO: implement predicate parsing [expr]
    // For now, return empty predicates
    Ok((input, vec![]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_path() {
        let expr = parse_xpath("/root/child").unwrap();
        match expr {
            XPathExpr::LocationPath(path) => {
                assert!(path.absolute);
                assert_eq!(path.steps.len(), 2);
                assert_eq!(path.steps[0].node_test, NodeTest::Name("root".into()));
                assert_eq!(path.steps[1].node_test, NodeTest::Name("child".into()));
            }
            _ => panic!("Expected LocationPath"),
        }
    }

    #[test]
    fn test_descendant() {
        let expr = parse_xpath("//claim").unwrap();
        match expr {
            XPathExpr::LocationPath(path) => {
                assert!(path.absolute);
                // First step is descendant-or-self::node()
                assert_eq!(path.steps[0].axis, Axis::DescendantOrSelf);
                assert_eq!(path.steps[1].node_test, NodeTest::Name("claim".into()));
            }
            _ => panic!("Expected LocationPath"),
        }
    }

    #[test]
    fn test_text_node() {
        let expr = parse_xpath("//claim/text()").unwrap();
        match expr {
            XPathExpr::LocationPath(path) => {
                let last = path.steps.last().unwrap();
                assert_eq!(last.node_test, NodeTest::Text);
            }
            _ => panic!("Expected LocationPath"),
        }
    }

    #[test]
    fn test_attribute() {
        let expr = parse_xpath("/root/@lang").unwrap();
        match expr {
            XPathExpr::LocationPath(path) => {
                let last = path.steps.last().unwrap();
                assert_eq!(last.axis, Axis::Attribute);
                assert_eq!(last.node_test, NodeTest::Name("lang".into()));
            }
            _ => panic!("Expected LocationPath"),
        }
    }

    #[test]
    fn test_wildcard() {
        let expr = parse_xpath("/root/*").unwrap();
        match expr {
            XPathExpr::LocationPath(path) => {
                let last = path.steps.last().unwrap();
                assert_eq!(last.node_test, NodeTest::Wildcard);
            }
            _ => panic!("Expected LocationPath"),
        }
    }

    #[test]
    fn test_parent_axis() {
        let expr = parse_xpath("..").unwrap();
        match expr {
            XPathExpr::LocationPath(path) => {
                assert_eq!(path.steps[0].axis, Axis::Parent);
            }
            _ => panic!("Expected LocationPath"),
        }
    }

    #[test]
    fn test_explicit_axis() {
        let expr = parse_xpath("ancestor::div").unwrap();
        match expr {
            XPathExpr::LocationPath(path) => {
                assert_eq!(path.steps[0].axis, Axis::Ancestor);
                assert_eq!(path.steps[0].node_test, NodeTest::Name("div".into()));
            }
            _ => panic!("Expected LocationPath"),
        }
    }
}
