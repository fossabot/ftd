#[derive(Debug, PartialEq, Clone, serde::Serialize, serde::Deserialize)]
pub struct OrType {
    pub name: String,
    pub variants: Vec<ftd::interpreter::Record>,
}

impl OrType {
    pub fn from_p1(p1: &ftd::p11::Section, doc: &ftd::interpreter::TDoc) -> ftd::p11::Result<Self> {
        let name = doc.format_name(p1.name.as_str());
        let mut variants: Vec<ftd::interpreter::Record> = Default::default();
        for s in p1.sub_sections.iter() {
            if s.is_commented {
                continue;
            }
            variants.push(ftd::interpreter::Record::from_p1(
                format!("record {}.{}", p1.name, s.name.as_str()).as_str(),
                &s.headers,
                doc,
                p1.line_number,
            )?);
        }
        Ok(OrType { name, variants })
    }

    pub fn create(
        &self,
        p1: &ftd::p11::Section,
        variant: String,
        doc: &ftd::interpreter::TDoc,
    ) -> ftd::p11::Result<ftd::interpreter::PropertyValue> {
        // todo: check if the its reference to other variable
        for v in self.variants.iter() {
            if v.name
                == doc.resolve_name(
                    p1.line_number,
                    format!("{}.{}", self.name, variant.as_str()).as_str(),
                )?
            {
                return Ok(ftd::interpreter::PropertyValue::Value {
                    value: ftd::interpreter::Value::OrType {
                        variant,
                        name: self.name.to_string(),
                        fields: v.fields(p1, doc)?,
                    },
                });
            }
        }

        ftd::interpreter::utils::e2(
            format!("{} is not a valid variant for {}", variant, self.name),
            doc.name,
            p1.line_number,
        )
    }
}

#[cfg(test)]
mod test {
    use ftd::test::*;

    #[test]
    fn basic() {
        let mut bag = ftd::interpreter::default_bag();

        bag.insert(s("foo/bar#entity"), entity());
        bag.insert(
            s("foo/bar#abrar"),
            ftd::interpreter::Thing::Variable(ftd::Variable {
                name: s("abrar"),
                flags: ftd::VariableFlags::default(),
                value: ftd::interpreter::PropertyValue::Value {
                    value: ftd::interpreter::Value::OrType {
                        name: s("foo/bar#entity"),
                        variant: s("person"),
                        fields: abrar(),
                    },
                },
                conditions: vec![],
            }),
        );
        bag.insert(
            "foo/bar#x".to_string(),
            ftd::interpreter::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: "x".to_string(),
                value: ftd::interpreter::PropertyValue::Value {
                    value: ftd::interpreter::Value::Integer { value: 10 },
                },
                conditions: vec![],
            }),
        );

        p!(
            "
            -- integer x: 10

            -- or-type entity:

            --- person:
            caption name:
            string address:
            body bio:
            integer age:

            --- company:
            caption name:
            string industry:

            -- entity.person abrar: Abrar Khan2
            age: $x
            address: Bihar2

            Software developer working at fifthtry2.
            ",
            (bag, default_column()),
        );
    }
}