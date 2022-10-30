#![allow(dead_code)]

#[derive(Debug, PartialEq, Clone, serde::Serialize, serde::Deserialize)]
pub enum Boolean {
    // if: $caption is not null
    IsNotNull {
        value: ftd::interpreter2::PropertyValue,
        line_number: usize,
    },
    // if: $caption is null
    IsNull {
        value: ftd::interpreter2::PropertyValue,
        line_number: usize,
    },
    // if: $list is not empty
    IsNotEmpty {
        value: ftd::interpreter2::PropertyValue,
        line_number: usize,
    },
    // if: $list is empty
    IsEmpty {
        value: ftd::interpreter2::PropertyValue,
        line_number: usize,
    },
    // if: $caption == hello | if: $foo
    Equal {
        left: ftd::interpreter2::PropertyValue,
        right: ftd::interpreter2::PropertyValue,
        line_number: usize,
    },
    // if: $caption != hello
    NotEqual {
        left: ftd::interpreter2::PropertyValue,
        right: ftd::interpreter2::PropertyValue,
        line_number: usize,
    },
    // if: false
    Literal {
        value: bool,
        line_number: usize,
    },
}

impl Boolean {
    pub(crate) fn from_ast_condition(
        condition: ftd::ast::Condition,
        definition_name_with_arguments: Option<(&str, &[ftd::interpreter2::Argument])>,
        loop_object_name_and_kind: &Option<(String, ftd::interpreter2::Argument)>,
        doc: &ftd::interpreter2::TDoc,
    ) -> ftd::interpreter2::Result<Boolean> {
        let (boolean, mut left, mut right) = Boolean::boolean_left_right(
            condition.line_number,
            condition.expression.as_str(),
            doc.name,
        )?;
        left = doc.resolve_reference_name(left.as_str(), condition.line_number)?;
        if let Some(ref r) = right {
            right = doc.resolve_reference_name(r, condition.line_number).ok();
        }

        Ok(match boolean.as_str() {
            "Literal" => Boolean::Literal {
                value: left == "true",
                line_number: condition.line_number,
            },
            "IsNotNull" | "IsNull" => {
                let value = ftd::interpreter2::PropertyValue::from_string_with_argument(
                    left.as_str(),
                    doc,
                    None,
                    false,
                    condition.line_number,
                    definition_name_with_arguments,
                    loop_object_name_and_kind,
                )?;
                if !value.kind().is_optional() {
                    return ftd::interpreter2::utils::e2(
                        format!("'{}' is not to a list", left),
                        doc.name,
                        condition.line_number,
                    );
                }

                if boolean.as_str() == "IsNotNull" {
                    Boolean::IsNotNull {
                        value,
                        line_number: condition.line_number,
                    }
                } else {
                    Boolean::IsNull {
                        value,
                        line_number: condition.line_number,
                    }
                }
            }
            "IsNotEmpty" | "IsEmpty" => {
                let value = ftd::interpreter2::PropertyValue::from_string_with_argument(
                    left.as_str(),
                    doc,
                    None,
                    false,
                    condition.line_number,
                    definition_name_with_arguments,
                    loop_object_name_and_kind,
                )?;
                if !value.kind().is_list() {
                    return ftd::interpreter2::utils::e2(
                        format!("'{}' is not to a list", left),
                        doc.name,
                        condition.line_number,
                    );
                }
                if boolean.as_str() == "IsNotEmpty" {
                    Boolean::IsNotEmpty {
                        value,
                        line_number: condition.line_number,
                    }
                } else {
                    Boolean::IsEmpty {
                        value,
                        line_number: condition.line_number,
                    }
                }
            }
            "NotEqual" | "Equal" => {
                if let Some(right) = right {
                    let left = ftd::interpreter2::PropertyValue::from_string_with_argument(
                        left.as_str(),
                        doc,
                        None,
                        false,
                        condition.line_number,
                        definition_name_with_arguments,
                        loop_object_name_and_kind,
                    )?;
                    let right = ftd::interpreter2::PropertyValue::from_string_with_argument(
                        right.as_str(),
                        doc,
                        Some(&ftd::interpreter2::KindData {
                            kind: left.kind(),
                            caption: false,
                            body: false,
                        }),
                        false,
                        condition.line_number,
                        definition_name_with_arguments,
                        loop_object_name_and_kind,
                    )?;
                    Boolean::Equal {
                        left,
                        right,
                        line_number: condition.line_number,
                    }
                } else {
                    Boolean::Equal {
                        left: ftd::interpreter2::PropertyValue::from_string_with_argument(
                            left.as_str(),
                            doc,
                            None,
                            false,
                            condition.line_number,
                            definition_name_with_arguments,
                            loop_object_name_and_kind,
                        )?,
                        right: ftd::interpreter2::PropertyValue::Value {
                            value: ftd::interpreter2::Value::Boolean {
                                value: boolean.as_str() == "Equal",
                            },
                            is_mutable: false,
                            line_number: condition.line_number,
                        },
                        line_number: condition.line_number,
                    }
                }
            }
            _ => {
                return ftd::interpreter2::utils::e2(
                    format!("'{}' is not valid condition", condition.expression),
                    doc.name,
                    condition.line_number,
                )
            }
        })
    }

    pub fn boolean_left_right(
        line_number: usize,
        expr: &str,
        doc_id: &str,
    ) -> ftd::interpreter2::Result<(String, String, Option<String>)> {
        let expr: String = expr.split_whitespace().collect::<Vec<&str>>().join(" ");
        if expr == "true" || expr == "false" {
            return Ok(("Literal".to_string(), expr, None));
        }
        let (left, rest) = match expr.split_once(' ') {
            None => return Ok(("Equal".to_string(), expr.to_string(), None)),
            Some(v) => v,
        };
        if left == "not" {
            return Ok(("NotEqual".to_string(), rest.to_string(), None));
        }
        Ok(match rest {
            "is not null" => ("IsNotNull".to_string(), left.to_string(), None),
            "is null" => ("IsNull".to_string(), left.to_string(), None),
            "is not empty" => ("IsNotEmpty".to_string(), left.to_string(), None),
            "is empty" => ("IsEmpty".to_string(), left.to_string(), None),
            _ if rest.starts_with("==") => (
                "Equal".to_string(),
                left.to_string(),
                Some(rest.replace("==", "").trim().to_string()),
            ),
            _ => {
                return ftd::interpreter2::utils::e2(
                    format!("'{}' is not valid condition", rest),
                    doc_id,
                    line_number,
                )
            }
        })
    }

    pub fn eval(&self, doc: &ftd::interpreter2::TDoc) -> ftd::interpreter2::Result<bool> {
        Ok(match self {
            Boolean::IsNotNull { value, line_number } => {
                !value.clone().resolve(doc, *line_number)?.is_null()
            }
            Boolean::IsNull { value, line_number } => {
                value.clone().resolve(doc, *line_number)?.is_null()
            }
            Boolean::IsNotEmpty { value, line_number } => {
                !value.clone().resolve(doc, *line_number)?.is_empty()
            }
            Boolean::IsEmpty { value, line_number } => {
                value.clone().resolve(doc, *line_number)?.is_empty()
            }
            Boolean::Equal {
                left,
                right,
                line_number,
            } => left
                .clone()
                .resolve(doc, *line_number)?
                .is_equal(&right.clone().resolve(doc, *line_number)?),
            Boolean::NotEqual {
                left,
                right,
                line_number,
            } => left
                .clone()
                .resolve(doc, *line_number)?
                .is_equal(&right.clone().resolve(doc, *line_number)?),
            Boolean::Literal { value, .. } => *value,
        })
    }
}