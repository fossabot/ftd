#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum Kind {
    String {
        caption: bool,
        body: bool,
        default: Option<String>,
        is_reference: bool,
    },
    Object {
        default: Option<String>,
        is_reference: bool,
    },
    Integer {
        default: Option<String>,
        is_reference: bool,
    },
    Decimal {
        default: Option<String>,
        is_reference: bool,
    },
    Boolean {
        default: Option<String>,
        is_reference: bool,
    },
    Element,
    Elements,
    Message,
    StringMessage, // message that takes a string
    IntMessage,    // message that takes an int
    Record {
        name: String,
        default: Option<String>,
        is_reference: bool,
    }, // the full name of the record (full document name.record name)
    OrType {
        name: String,
        is_reference: bool,
    }, // the full name of the or-type
    OrTypeWithVariant {
        name: String,
        variant: String,
        is_reference: bool,
    },
    Map {
        kind: Box<Kind>,
        is_reference: bool,
    }, // map of String to Kind
    List {
        kind: Box<Kind>,
        default: Option<String>,
        is_reference: bool,
    },
    Optional {
        kind: Box<Kind>,
        is_reference: bool,
    },
    UI {
        // TODO: Option<ftd::p11::Header>
        default: Vec<ftd::p11::Header>,
    },
}

impl Kind {
    pub fn is_reference(&self) -> bool {
        match self {
            Kind::String { is_reference, .. }
            | Kind::Object { is_reference, .. }
            | Kind::Integer { is_reference, .. }
            | Kind::Decimal { is_reference, .. }
            | Kind::Boolean { is_reference, .. }
            | Kind::Record { is_reference, .. }
            | Kind::OrType { is_reference, .. }
            | Kind::OrTypeWithVariant { is_reference, .. }
            | Kind::Map { is_reference, .. }
            | Kind::List { is_reference, .. }
            | Kind::Optional { is_reference, .. } => *is_reference,
            _ => false,
        }
    }

    pub fn is_string(&self) -> bool {
        matches!(self, Kind::String { .. })
    }

    pub fn is_decimal(&self) -> bool {
        matches!(self, Kind::Decimal { .. })
    }

    pub fn is_integer(&self) -> bool {
        matches!(self, Kind::Integer { .. })
    }

    pub fn is_boolean(&self) -> bool {
        matches!(self, Kind::Boolean { .. })
    }

    pub fn is_optional(&self) -> bool {
        matches!(self, Kind::Optional { .. })
    }

    pub fn is_list(&self) -> bool {
        matches!(self, Kind::List { .. })
    }

    pub fn is_record(&self) -> bool {
        matches!(self, Kind::Record { .. })
    }

    pub fn to_string(&self, line_number: usize, doc_id: &str) -> ftd::p11::Result<String> {
        Ok(match self.inner() {
            ftd::interpreter::Kind::String { .. } => "string",
            ftd::interpreter::Kind::Integer { .. } => "integer",
            ftd::interpreter::Kind::Decimal { .. } => "decimal",
            ftd::interpreter::Kind::Boolean { .. } => "boolean",
            ftd::interpreter::Kind::Object { .. } => "object",
            ftd::interpreter::Kind::List { .. } => "list",
            _ => return ftd::interpreter::utils::e2(format!("1 Kind supported for default value are string, integer, decimal and boolean with default value, found: kind `{:?}`", &self), doc_id, line_number),
        }.to_string())
    }

    pub fn to_value(
        &self,
        line_number: usize,
        doc_id: &str,
    ) -> ftd::p11::Result<ftd::interpreter::Value> {
        Ok(match self {
            ftd::interpreter::Kind::String { default: Some(d), .. } => ftd::interpreter::Value::String {text: d.to_string(), source: ftd::interpreter::TextSource::Default} ,
            ftd::interpreter::Kind::Integer { default: Some(d), .. } => ftd::interpreter::Value::Integer { value: match d.parse::<i64>() {
                    Ok(v) => v,
                    Err(_) => return ftd::interpreter::utils::e2(format!("{} is not an integer", d), doc_id, line_number),
                },
            },
            ftd::interpreter::Kind::Decimal { default: Some(d), .. } => ftd::interpreter::Value::Decimal { value: d.parse::<f64>().map_err(|e| ftd::p11::Error::ParseError {
                    message: e.to_string(),
                    doc_id: doc_id.to_string(),
                    line_number,
                })?,
            },
            ftd::interpreter::Kind::Boolean { default: Some(d), .. } => ftd::interpreter::Value::Boolean { value: d.parse::<bool>().map_err(|e|ftd::p11::Error::ParseError {
                    message: e.to_string(),
                    doc_id: doc_id.to_string(),
                    line_number,
                })?,
            },
            ftd::interpreter::Kind::Optional {kind, ..} => if let Ok(f) = kind.to_value(line_number, doc_id) {
                ftd::interpreter::Value::Optional {data: Box::new(Some(f)), kind: kind.as_ref().to_owned()}
            } else {
                ftd::interpreter::Value::Optional {data: Box::new(None), kind: kind.as_ref().to_owned()}
            },
            ftd::interpreter::Kind::List { kind, .. } => ftd::interpreter::Value::List { data: vec![], kind: kind.as_ref().to_owned() },
            _ => return ftd::interpreter::utils::e2(
                format!("2 Kind supported for default value are string, integer, decimal and boolean with default value, found: kind `{:?}`", &self),
                doc_id,  line_number),
        })
    }

    pub fn has_default_value(&self) -> bool {
        match self {
            Kind::String { default, .. }
            | Kind::Integer { default, .. }
            | Kind::Decimal { default, .. }
            | Kind::Boolean { default, .. }
            | Kind::Record { default, .. }
            | Kind::List { default, .. } => default.is_some(),
            Kind::UI { default } => default.is_some(),
            _ => false,
        }
    }
}

impl Kind {
    pub fn is_same_as(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::String { .. }, Self::String { .. }) => matches!(other, Self::String { .. }),
            (Self::UI { .. }, Self::UI { .. }) => matches!(other, Self::UI { .. }),
            (Self::Optional { kind, .. }, _) => kind.is_same_as(other),
            (_, Self::Optional { kind: other, .. }) => self.is_same_as(other),
            _ => self.without_default() == other.without_default(),
        }
    }

    pub fn without_default(&self) -> Self {
        match self {
            Kind::Integer { .. } => Kind::Integer {
                default: None,
                is_reference: false,
            },
            Kind::Boolean { .. } => Kind::Boolean {
                default: None,
                is_reference: false,
            },
            Kind::Decimal { .. } => Kind::Decimal {
                default: None,
                is_reference: false,
            },
            Kind::String { caption, body, .. } => Kind::String {
                caption: *caption,
                body: *body,
                default: None,
                is_reference: false,
            },
            Kind::Record { name, .. } => Kind::Record {
                name: name.clone(),
                default: None,
                is_reference: false,
            },
            Kind::List { kind, .. } => Kind::List {
                kind: kind.clone(),
                default: None,
                is_reference: false,
            },
            _ => self.clone(),
        }
    }

    pub fn record(name: &str) -> Self {
        Kind::Record {
            name: name.to_string(),
            default: None,
            is_reference: false,
        }
    }

    pub fn integer() -> Self {
        Kind::Integer {
            default: None,
            is_reference: false,
        }
    }

    pub fn decimal() -> Self {
        Kind::Decimal {
            default: None,
            is_reference: false,
        }
    }

    pub fn boolean() -> Self {
        Kind::Boolean {
            default: None,
            is_reference: false,
        }
    }

    pub fn object() -> Self {
        Kind::Object {
            default: Default::default(),
            is_reference: false,
        }
    }

    pub fn string() -> Self {
        Kind::String {
            caption: false,
            body: false,
            default: None,
            is_reference: false,
        }
    }
    pub fn get_default_value_str(&self) -> Option<String> {
        match self {
            Kind::Integer { default, .. }
            | Kind::Boolean { default, .. }
            | Kind::Decimal { default, .. }
            | Kind::Record { default, .. }
            | Kind::List { default, .. }
            | Kind::String { default, .. } => default.clone(),
            Kind::UI { default, .. } => default.as_ref().map(|(v, _)| v.clone()),
            Kind::Optional { kind, .. } => kind.get_default_value_str(),
            _ => None,
        }
    }

    pub fn set_default(self, default: Option<String>) -> Self {
        match self {
            Kind::String {
                caption,
                body,
                is_reference,
                ..
            } => Kind::String {
                caption,
                body,
                default,
                is_reference,
            },
            Kind::Record {
                name, is_reference, ..
            } => Kind::Record {
                name,
                default,
                is_reference,
            },
            Kind::UI { .. } => Kind::UI {
                default: default.map(|v| (v, Default::default())),
            },
            Kind::Integer { is_reference, .. } => Kind::Integer {
                default,
                is_reference,
            },
            Kind::Decimal { is_reference, .. } => Kind::Decimal {
                is_reference,
                default,
            },
            Kind::Boolean { is_reference, .. } => Kind::Boolean {
                is_reference,
                default,
            },
            Kind::Optional { is_reference, kind } => Kind::Optional {
                kind: Box::from(kind.set_default(default)),
                is_reference,
            },
            Kind::List {
                is_reference, kind, ..
            } => Kind::List {
                is_reference,
                kind,
                default,
            },
            _ => self,
        }
    }

    pub fn set_reference(self, is_reference: bool) -> Self {
        match self {
            Kind::String {
                caption,
                body,
                default,
                ..
            } => Kind::String {
                caption,
                body,
                default,
                is_reference,
            },
            Kind::Record { name, default, .. } => Kind::Record {
                name,
                default,
                is_reference,
            },
            Kind::Integer { default, .. } => Kind::Integer {
                default,
                is_reference,
            },
            Kind::Decimal { default, .. } => Kind::Decimal {
                is_reference,
                default,
            },
            Kind::Boolean { default, .. } => Kind::Boolean {
                is_reference,
                default,
            },
            Kind::Optional { kind, .. } => Kind::Optional { kind, is_reference },
            Kind::List { default, kind, .. } => Kind::List {
                is_reference,
                kind,
                default,
            },
            _ => self,
        }
    }

    pub fn caption() -> Self {
        Kind::String {
            caption: true,
            body: false,
            default: None,
            is_reference: false,
        }
    }

    pub fn body() -> Self {
        Kind::String {
            caption: false,
            body: true,
            default: None,
            is_reference: false,
        }
    }

    pub fn caption_or_body() -> Self {
        Kind::String {
            caption: true,
            body: true,
            default: None,
            is_reference: false,
        }
    }

    pub fn optional(k: Self) -> Self {
        Kind::Optional {
            kind: Box::new(k),
            is_reference: false,
        }
    }

    pub fn list(k: Self) -> Self {
        Kind::List {
            kind: Box::new(k),
            default: None,
            is_reference: false,
        }
    }

    pub fn map(k: Self) -> Self {
        Kind::Map {
            kind: Box::new(k),
            is_reference: false,
        }
    }

    pub fn into_optional(self) -> Self {
        Kind::Optional {
            kind: Box::new(self),
            is_reference: false,
        }
    }

    pub fn inner(&self) -> &Self {
        match self {
            Kind::Optional { kind, .. } => kind,
            _ => self,
        }
    }

    pub fn mut_inner(&mut self) -> &mut Self {
        match self {
            Kind::Optional { kind, .. } => kind,
            _ => self,
        }
    }

    pub fn list_kind(&self) -> &Self {
        match self {
            Kind::List { kind, .. } => kind,
            _ => self,
        }
    }

    pub fn string_any(&self) -> Self {
        match self {
            Self::String { .. } => Self::String {
                caption: true,
                body: true,
                default: None,
                is_reference: false,
            },
            _ => self.to_owned(),
        }
    }

    pub fn read_section(
        &self,
        line_number: usize,
        p1: ftd::p11::Headers,
        p1_caption: &Option<ftd::p11::Header>,
        p1_body: &Option<ftd::p11::Body>,
        name: &str,
        doc: &ftd::interpreter::TDoc,
    ) -> ftd::p11::Result<ftd::interpreter::PropertyValue> {
        let (v, source) = match p1.str_optional(doc.name, line_number, name)? {
            Some(v) => (v.to_string(), ftd::interpreter::TextSource::Header),
            None => {
                let optional = match self {
                    Kind::Optional { kind, .. } => match kind.as_ref() {
                        ftd::interpreter::Kind::String { .. }
                        | ftd::interpreter::Kind::Integer { .. }
                        | ftd::interpreter::Kind::Decimal { .. }
                        | ftd::interpreter::Kind::Boolean { .. } => true,
                        _ => {
                            return Ok(ftd::interpreter::PropertyValue::Value {
                                value: ftd::interpreter::Value::None {
                                    kind: *kind.clone(),
                                },
                            })
                        }
                    },
                    ftd::interpreter::Kind::String { .. }
                    | ftd::interpreter::Kind::Integer { .. }
                    | ftd::interpreter::Kind::Decimal { .. }
                    | ftd::interpreter::Kind::Boolean { .. } => false,
                    t => {
                        return ftd::interpreter::utils::e2(
                            format!("`{}` is {:?}", name, t),
                            doc.name,
                            line_number,
                        )
                    }
                };

                let (caption, body) = if let Kind::String { caption, body, .. } = self.inner() {
                    (*caption, *body)
                } else {
                    (false, false)
                };

                if caption && p1_caption.is_some() {
                    (
                        p1_caption.as_ref().expect("asd").to_string(),
                        ftd::interpreter::TextSource::Caption,
                    )
                } else if body && p1_body.is_some() {
                    (
                        p1_body.as_ref().expect("asd").1.to_string(),
                        ftd::interpreter::TextSource::Body,
                    )
                } else if optional {
                    return Ok(ftd::interpreter::PropertyValue::Value {
                        value: ftd::interpreter::Value::None {
                            kind: self.inner().to_owned(),
                        },
                    });
                } else if let Some(default) = self.get_default_value_str() {
                    (default, ftd::interpreter::TextSource::Default)
                } else {
                    return ftd::interpreter::utils::e2(
                        format!("`{}` is required", name),
                        doc.name,
                        line_number,
                    );
                }
            }
        };

        if v.starts_with('$') {
            return ftd::interpreter::PropertyValue::resolve_value(
                line_number,
                &v,
                Some(self.to_owned()),
                doc,
                &Default::default(),
                None,
            );
        }

        match self.inner() {
            Kind::Integer { .. } => Ok(ftd::interpreter::PropertyValue::Value {
                value: ftd::interpreter::Value::Integer {
                    value: p1.i64(doc.name, line_number, name).unwrap_or(
                        v.parse::<i64>().map_err(|e| ftd::p11::Error::ParseError {
                            message: e.to_string(),
                            doc_id: doc.name.to_string(),
                            line_number,
                        })?,
                    ),
                },
            }),
            Kind::Decimal { .. } => Ok(ftd::interpreter::PropertyValue::Value {
                value: ftd::interpreter::Value::Decimal {
                    value: p1.f64(doc.name, line_number, name).unwrap_or(
                        v.parse::<f64>().map_err(|e| ftd::p11::Error::ParseError {
                            message: e.to_string(),
                            doc_id: doc.name.to_string(),
                            line_number,
                        })?,
                    ),
                },
            }),
            Kind::Boolean { .. } => Ok(ftd::interpreter::PropertyValue::Value {
                value: ftd::interpreter::Value::Boolean {
                    value: p1.bool(doc.name, line_number, name).unwrap_or(
                        v.parse::<bool>().map_err(|e| ftd::p11::Error::ParseError {
                            message: e.to_string(),
                            doc_id: doc.name.to_string(),
                            line_number,
                        })?,
                    ),
                },
            }),
            Kind::String { .. } => Ok(ftd::interpreter::PropertyValue::Value {
                value: ftd::interpreter::Value::String { text: v, source },
            }),
            v => ftd::interpreter::utils::e2(
                format!("unknown kind found: {:?}", v),
                doc.name,
                line_number,
            ),
        }
    }

    pub fn from(
        line_number: usize,
        s: &str,
        doc: &ftd::interpreter::TDoc,
        object_kind: Option<(&str, Self)>,
    ) -> ftd::p11::Result<Self> {
        let (optional, k) = if s.starts_with("optional ") {
            (
                true,
                ftd::interpreter::utils::get_name("optional", s, doc.name)?,
            )
        } else {
            (false, s)
        };

        if k.starts_with("list ") {
            return Ok(Kind::List {
                kind: Box::new(Self::from(
                    line_number,
                    ftd::interpreter::utils::get_name("list", k, doc.name)?,
                    doc,
                    object_kind,
                )?),
                default: None,
                is_reference: false,
            });
        }

        if let Some((obj_name, obj_kind)) = object_kind {
            if k == obj_name {
                return Ok(obj_kind);
            }
        }

        let (key, default) = {
            if k.contains("with default") {
                let mut parts = k.splitn(2, " with default");
                let k = parts.next().unwrap().trim();
                let d = parts.next().unwrap().trim();
                (k, Some(d.to_string()))
            } else {
                (k, None)
            }
        };

        let k = match key {
            "string" => Kind::string(),
            "caption" => Kind::caption(),
            "body" => Kind::body(),
            "body or caption" | "caption or body" => Kind::caption_or_body(),
            "integer" => Kind::integer(),
            "decimal" => Kind::decimal(),
            "object" => Kind::object(),
            "boolean" => Kind::boolean(),
            "element" => Kind::Element,
            "elements" => Kind::Elements,
            "message" => Kind::Message,
            "string-message" => Kind::StringMessage,
            "int-message" => Kind::IntMessage,
            "ftd.ui" => Kind::UI { default: None },
            _ => match doc.get_thing(line_number, k)? {
                ftd::interpreter::Thing::Record(r) => Kind::Record {
                    name: r.name,
                    default: None,
                    is_reference: false,
                },
                ftd::interpreter::Thing::OrType(e) => Kind::OrType {
                    name: e.name,
                    is_reference: false,
                },
                t => unimplemented!(
                    "{} is {:?}, line number: {}, doc: {}",
                    k,
                    t,
                    line_number,
                    doc.name.to_string()
                ),
            },
        }
        .set_default(default);

        Ok(if optional { Self::optional(k) } else { k })
    }

    pub fn for_variable(
        line_number: usize,
        s: &str,
        kind: &Option<String>,
        default: Option<String>, // TODO: default to take section
        doc: &ftd::interpreter::TDoc,
        object_kind: Option<(&str, Self)>,
        arguments: &ftd::Map<ftd::interpreter::Kind>,
    ) -> ftd::p11::Result<Self> {
        let default = {
            // resolve the default value
            let mut default = default;
            if let Some(ref v) = default {
                default = Some(doc.resolve_reference_name(line_number, v, arguments)?);
            }
            default
        };

        let var_data = ftd::interpreter::variable::VariableData::get_name_kind(
            s,
            kind,
            doc,
            line_number,
            vec![].as_slice(),
        )?;

        let k = match object_kind {
            Some(object_kind) if var_data.kind.eq(object_kind.0) => object_kind.1,
            _ => match var_data.kind.as_str() {
                "string" => Kind::string(),
                "caption" => Kind::caption(),
                "body" => Kind::body(),
                "body or caption" | "caption or body" => Kind::caption_or_body(),
                "integer" => Kind::integer(),
                "decimal" => Kind::decimal(),
                "object" => Kind::object(),
                "boolean" => Kind::boolean(),
                "element" => Kind::Element,
                "elements" => Kind::Elements,
                "message" => Kind::Message,
                "string-message" => Kind::StringMessage,
                "int-message" => Kind::IntMessage,
                "ftd.ui" => Kind::UI { default: None },
                k => match doc.get_thing(line_number, k) {
                    Ok(ftd::interpreter::Thing::Record(r)) => Kind::Record {
                        name: r.name,
                        default: None,
                        is_reference: false,
                    },
                    Ok(ftd::interpreter::Thing::OrType(e)) => Kind::OrType {
                        name: e.name,
                        is_reference: false,
                    },
                    t => match default {
                        None => unimplemented!(
                            "{} is {:?}, line number: {}, doc: {}",
                            var_data.name,
                            t,
                            line_number,
                            doc.name.to_string()
                        ),
                        Some(ref d) => ftd::interpreter::variable::guess_type(d, false)?.kind(),
                    },
                },
            },
        };

        if var_data.is_list() {
            return Ok(Kind::List {
                kind: Box::new(k),
                default,
                is_reference: var_data.is_reference,
            });
        }

        Ok(if var_data.is_optional() {
            Self::optional(k.set_default(default))
        } else {
            k.set_default(default)
        }
        .set_reference(var_data.is_reference))
    }
}