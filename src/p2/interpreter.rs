pub(crate) struct Interpreter<'a> {
    lib: &'a dyn ftd::p2::Library,
    pub bag: std::collections::BTreeMap<String, ftd::p2::Thing>,
    pub p1: Vec<ftd::p1::Section>,
    pub aliases: std::collections::BTreeMap<String, String>,
    pub parsed_libs: Vec<String>,
}

impl<'a> Interpreter<'a> {
    #[cfg(feature = "async")]
    pub(crate) fn interpret(
        &mut self,
        name: &str,
        s: &str,
    ) -> ftd::p1::Result<Vec<ftd::Instruction>> {
        futures::executor::block_on(self.async_interpret(name, s))
    }

    // #[observed(with_result, namespace = "ftd")]
    #[cfg(feature = "async")]
    pub(crate) async fn async_interpret(
        &mut self,
        name: &str,
        s: &str,
    ) -> ftd::p1::Result<Vec<ftd::Instruction>> {
        let mut d_get = std::time::Duration::new(0, 0);
        let mut d_processor = std::time::Duration::new(0, 0);
        let v = self
            .async_interpret_(name, s, true, &mut d_get, &mut d_processor)
            .await?;
        // observer::observe_string("time_get", elapsed(d_get).as_str());
        // observer::observe_string("time_processor", elapsed(d_processor).as_str());
        Ok(v)
    }

    #[cfg(not(feature = "async"))]
    pub(crate) fn interpret(
        &mut self,
        name: &str,
        s: &str,
    ) -> ftd::p1::Result<Vec<ftd::Instruction>> {
        let mut d_get = std::time::Duration::new(0, 0);
        let mut d_processor = std::time::Duration::new(0, 0);
        let v = self.interpret_(name, s, true, &mut d_get, &mut d_processor)?;
        // observer::observe_string("time_get", elapsed(d_get).as_str());
        // observer::observe_string("time_processor", elapsed(d_processor).as_str());
        Ok(v)
    }

    fn library_in_the_bag(&self, name: &str) -> bool {
        self.parsed_libs.contains(&name.to_string())
    }

    fn add_library_to_bag(&mut self, name: &str) {
        if !self.library_in_the_bag(name) {
            self.parsed_libs.push(name.to_string());
        }
    }

    #[cfg(feature = "async")]
    #[async_recursion::async_recursion(?Send)]
    async fn async_interpret_(
        &mut self,
        name: &str,
        s: &str,
        is_main: bool,
        d_get: &mut std::time::Duration,
        d_processor: &mut std::time::Duration,
    ) -> ftd::p1::Result<Vec<ftd::Instruction>> {
        let p1 = ftd::p1::parse(s, name)?;
        let (new_p1, var_types) = ftd::p2::utils::reorder(&p1, name)?;

        let mut aliases = default_aliases();
        let mut instructions: Vec<ftd::Instruction> = Default::default();

        for p1 in new_p1.iter() {
            if p1.is_commented {
                continue;
            }
            if p1.name == "import" {
                let (library_name, alias) =
                    ftd::p2::utils::parse_import(&p1.caption, name, p1.line_number)?;
                aliases.insert(alias, library_name.clone());
                let start = std::time::Instant::now();
                let s = self.lib.get_with_result(library_name.as_str()).await?;
                *d_get = d_get.saturating_add(std::time::Instant::now() - start);
                if !self.library_in_the_bag(library_name.as_str()) {
                    self.async_interpret_(
                        library_name.as_str(),
                        s.as_str(),
                        false,
                        d_get,
                        d_processor,
                    )
                    .await?;
                    self.add_library_to_bag(library_name.as_str())
                }
                continue;
            }

            // while this is a specific to entire document, we are still creating it in a loop
            // because otherwise the self.interpret() call wont compile.
            let doc = ftd::p2::TDoc {
                name,
                aliases: &aliases,
                bag: &self.bag,
                local_variables: Default::default(),
            };

            let var_data = ftd::variable::VariableData::get_name_kind(
                &p1.name,
                &doc,
                p1.line_number,
                &var_types,
            );

            let mut thing = vec![];

            if p1.name.starts_with("record ") {
                // declare a record
                let d =
                    ftd::p2::Record::from_p1(p1.name.as_str(), &p1.header, &doc, p1.line_number)?;
                thing.push((
                    doc.resolve_name(p1.line_number, &d.name.to_string())?,
                    ftd::p2::Thing::Record(d),
                ));
            } else if p1.name.starts_with("or-type ") {
                // declare a record
                let d = ftd::OrType::from_p1(p1, &doc)?;
                thing.push((
                    doc.resolve_name(p1.line_number, &d.name.to_string())?,
                    ftd::p2::Thing::OrType(d),
                ));
            } else if p1.name.starts_with("map ") {
                let d = ftd::Variable::map_from_p1(p1, &doc)?;
                thing.push((
                    doc.resolve_name(p1.line_number, &d.name.to_string())?,
                    ftd::p2::Thing::Variable(d),
                ));
                // } else if_two_words(p1.name.as_str() {
                //   TODO: <record-name> <variable-name>: foo can be used to create a variable/
                //         Not sure if its a good idea tho.
                // }
            } else if p1.name == "container" {
                instructions.push(ftd::Instruction::ChangeContainer {
                    name: doc.resolve_name_with_instruction(
                        p1.line_number,
                        p1.caption(p1.line_number, doc.name)?.as_str(),
                        &instructions,
                    )?,
                });
            } else if let Ok(ftd::variable::VariableData {
                type_: ftd::variable::Type::Component,
                ..
            }) = var_data
            {
                // declare a function
                let d = ftd::Component::from_p1(p1, &doc)?;
                thing.push((
                    doc.resolve_name(p1.line_number, &d.full_name.to_string())?,
                    ftd::p2::Thing::Component(d),
                ));
                // processed_p1.push(p1.name.to_string());
            } else if let Ok(ref var_data) = var_data {
                let d = if p1
                    .header
                    .str(doc.name, p1.line_number, "$processor$")
                    .is_ok()
                {
                    let name = doc.resolve_name(p1.line_number, &var_data.name)?;
                    let start = std::time::Instant::now();
                    let value = self.lib.process(p1, &doc).await?;
                    *d_processor = d_processor.saturating_add(std::time::Instant::now() - start);
                    ftd::Variable {
                        name,
                        value: ftd::PropertyValue::Value { value },
                        conditions: vec![],
                        flags: ftd::variable::VariableFlags::from_p1(
                            &p1.header,
                            doc.name,
                            p1.line_number,
                        )?,
                    }
                } else if var_data.is_none() || var_data.is_optional() {
                    // declare and instantiate a variable
                    ftd::Variable::from_p1(p1, &doc)?
                } else {
                    // declare and instantiate a list
                    ftd::Variable::list_from_p1(p1, &doc)?
                };
                thing.push((
                    doc.resolve_name(p1.line_number, &d.name.to_string())?,
                    ftd::p2::Thing::Variable(d),
                ));
            } else if let ftd::p2::Thing::Variable(mut v) =
                doc.get_thing(p1.line_number, p1.name.as_str())?
            {
                assert!(
                    !(p1.header
                        .str_optional(doc.name, p1.line_number, "if")?
                        .is_some()
                        && p1
                            .header
                            .str_optional(doc.name, p1.line_number, "$processor$")?
                            .is_some())
                );
                if let Some(expr) = p1.header.str_optional(doc.name, p1.line_number, "if")? {
                    let val = v.get_value(p1, &doc)?;
                    v.conditions.push((
                        ftd::p2::Boolean::from_expression(
                            expr,
                            &doc,
                            &Default::default(),
                            (None, None),
                            p1.line_number,
                        )?,
                        val,
                    ));
                } else if p1
                    .header
                    .str_optional(doc.name, p1.line_number, "$processor$")?
                    .is_some()
                {
                    let start = std::time::Instant::now();
                    let value = self.lib.process(p1, &doc).await?;
                    *d_processor = d_processor.saturating_add(std::time::Instant::now() - start);
                    v.value = ftd::PropertyValue::Value { value };
                } else {
                    v.update_from_p1(p1, &doc)?;
                }
                thing.push((
                    doc.resolve_name(p1.line_number, &p1.name.to_string())?,
                    ftd::p2::Thing::Variable(v),
                ));
            } else {
                // cloning because https://github.com/rust-lang/rust/issues/59159
                match (doc.get_thing(p1.line_number, p1.name.as_str())?).clone() {
                    ftd::p2::Thing::Variable(_) => {
                        return ftd::e2(
                            format!("variable should have prefix $, found: `{}`", p1.name),
                            doc.name,
                            p1.line_number,
                        );
                    }
                    ftd::p2::Thing::Component(_) => {
                        let p1 = {
                            let mut p1 = p1.clone();
                            if p1
                                .header
                                .str_optional(doc.name, p1.line_number, "$processor$")?
                                .is_some()
                            {
                                let value = self.lib.process(&p1, &doc).await?;
                                Self::p1_from_processor(&mut p1, value);
                            }
                            p1
                        };
                        if let Ok(loop_data) = p1.header.str(doc.name, p1.line_number, "$loop$") {
                            let section_to_subsection = ftd::p1::SubSection {
                                name: p1.name.to_string(),
                                caption: p1.caption.to_owned(),
                                header: p1.header.to_owned(),
                                body: p1.body.to_owned(),
                                is_commented: p1.is_commented,
                                line_number: p1.line_number,
                            };
                            instructions.push(ftd::Instruction::RecursiveChildComponent {
                                child: ftd::component::recursive_child_component(
                                    loop_data,
                                    &section_to_subsection,
                                    &doc,
                                    &Default::default(),
                                    None,
                                )?,
                            });
                        } else {
                            let parent = ftd::ChildComponent::from_p1(
                                p1.line_number,
                                p1.name.as_str(),
                                &p1.header,
                                &p1.caption,
                                &p1.body_without_comment(),
                                &doc,
                                &Default::default(),
                            )?;

                            let mut children = vec![];

                            for sub in p1.sub_sections.0.iter() {
                                if sub.is_commented {
                                    continue;
                                }
                                if let Ok(loop_data) =
                                    sub.header.str(doc.name, p1.line_number, "$loop$")
                                {
                                    children.push(ftd::component::recursive_child_component(
                                        loop_data,
                                        sub,
                                        &doc,
                                        &parent.arguments,
                                        None,
                                    )?);
                                } else {
                                    let child = if parent.root.eq("ftd#text") {
                                        ftd::p2::utils::get_markup_child(
                                            &sub,
                                            &doc,
                                            &parent.arguments,
                                        )?
                                    } else {
                                        ftd::ChildComponent::from_p1(
                                            sub.line_number,
                                            sub.name.as_str(),
                                            &sub.header,
                                            &sub.caption,
                                            &sub.body_without_comment(),
                                            &doc,
                                            &parent.arguments,
                                        )?
                                    };
                                    children.push(child);
                                }
                            }

                            instructions.push(ftd::Instruction::Component { children, parent })
                        }
                    }
                    ftd::p2::Thing::Record(mut r) => {
                        r.add_instance(p1, &doc)?;
                        thing.push((
                            doc.resolve_name(p1.line_number, &p1.name.to_string())?,
                            ftd::p2::Thing::Record(r),
                        ));
                    }
                    ftd::p2::Thing::OrType(_r) => {
                        // do we allow initialization of a record by name? nopes
                        return ftd::e2(
                            format!("'{}' is an or-type", p1.name.as_str()),
                            doc.name,
                            p1.line_number,
                        );
                    }
                    ftd::p2::Thing::OrTypeWithVariant { .. } => {
                        // do we allow initialization of a record by name? nopes
                        return ftd::e2(
                            format!("'{}' is an or-type variant", p1.name.as_str(),),
                            doc.name,
                            p1.line_number,
                        );
                    }
                };
            }
            self.bag.extend(thing);
        }

        if is_main {
            self.p1 = p1;
            self.aliases = aliases;
        }
        Ok(instructions)
    }

    // #[cfg(not(feature = "async"))]
    fn interpret_(
        &mut self,
        name: &str,
        s: &str,
        is_main: bool,
        d_get: &mut std::time::Duration,
        d_processor: &mut std::time::Duration,
    ) -> ftd::p1::Result<Vec<ftd::Instruction>> {
        let p1 = ftd::p1::parse(s, name)?;
        let (new_p1, var_types) = ftd::p2::utils::reorder(&p1, name)?;

        let mut aliases = default_aliases();
        let mut instructions: Vec<ftd::Instruction> = Default::default();

        for p1 in new_p1.iter() {
            if p1.is_commented {
                continue;
            }

            if p1.name == "import" {
                let (library_name, alias) =
                    ftd::p2::utils::parse_import(&p1.caption, name, p1.line_number)?;
                aliases.insert(alias, library_name.clone());
                let start = std::time::Instant::now();
                let s = self.lib.get_with_result(library_name.as_str())?;
                *d_get = d_get.saturating_add(std::time::Instant::now() - start);
                if !self.library_in_the_bag(library_name.as_str()) {
                    self.interpret_(library_name.as_str(), s.as_str(), false, d_get, d_processor)?;
                    self.add_library_to_bag(library_name.as_str())
                }
                continue;
            }

            // while this is a specific to entire document, we are still creating it in a loop
            // because otherwise the self.interpret() call wont compile.
            let doc = ftd::p2::TDoc {
                name,
                aliases: &aliases,
                bag: &self.bag,
                local_variables: Default::default(),
            };

            let var_data = ftd::variable::VariableData::get_name_kind(
                &p1.name,
                &doc,
                p1.line_number,
                &var_types,
            );

            let mut thing = vec![];

            if p1.name.starts_with("record ") {
                // declare a record
                let d =
                    ftd::p2::Record::from_p1(p1.name.as_str(), &p1.header, &doc, p1.line_number)?;
                thing.push((
                    doc.resolve_name(p1.line_number, &d.name.to_string())?,
                    ftd::p2::Thing::Record(d),
                ));
            } else if p1.name.starts_with("or-type ") {
                // declare a record
                let d = ftd::OrType::from_p1(p1, &doc)?;
                thing.push((
                    doc.resolve_name(p1.line_number, &d.name.to_string())?,
                    ftd::p2::Thing::OrType(d),
                ));
            } else if p1.name.starts_with("map ") {
                let d = ftd::Variable::map_from_p1(p1, &doc)?;
                thing.push((
                    doc.resolve_name(p1.line_number, &d.name.to_string())?,
                    ftd::p2::Thing::Variable(d),
                ));
                // } else if_two_words(p1.name.as_str() {
                //   TODO: <record-name> <variable-name>: foo can be used to create a variable/
                //         Not sure if its a good idea tho.
                // }
            } else if p1.name == "container" {
                instructions.push(ftd::Instruction::ChangeContainer {
                    name: doc.resolve_name_with_instruction(
                        p1.line_number,
                        p1.caption(p1.line_number, doc.name)?.as_str(),
                        &instructions,
                    )?,
                });
            } else if let Ok(ftd::variable::VariableData {
                type_: ftd::variable::Type::Component,
                ..
            }) = var_data
            {
                // declare a function
                let d = ftd::Component::from_p1(p1, &doc)?;
                thing.push((
                    doc.resolve_name(p1.line_number, &d.full_name.to_string())?,
                    ftd::p2::Thing::Component(d),
                ));
                // processed_p1.push(p1.name.to_string());
            } else if let Ok(ref var_data) = var_data {
                let d = if p1
                    .header
                    .str(doc.name, p1.line_number, "$processor$")
                    .is_ok()
                {
                    let name = doc.resolve_name(p1.line_number, &var_data.name)?;
                    let start = std::time::Instant::now();
                    let value = self.lib.process(p1, &doc)?;
                    *d_processor = d_processor.saturating_add(std::time::Instant::now() - start);
                    ftd::Variable {
                        name,
                        value: ftd::PropertyValue::Value { value },
                        conditions: vec![],
                        flags: ftd::variable::VariableFlags::from_p1(
                            &p1.header,
                            doc.name,
                            p1.line_number,
                        )?,
                    }
                } else if var_data.is_none() || var_data.is_optional() {
                    // declare and instantiate a variable
                    ftd::Variable::from_p1(p1, &doc)?
                } else {
                    // declare and instantiate a list
                    ftd::Variable::list_from_p1(p1, &doc)?
                };
                thing.push((
                    doc.resolve_name(p1.line_number, &d.name.to_string())?,
                    ftd::p2::Thing::Variable(d),
                ));
            } else if let ftd::p2::Thing::Variable(mut v) =
                doc.get_thing(p1.line_number, p1.name.as_str())?
            {
                assert!(
                    !(p1.header
                        .str_optional(doc.name, p1.line_number, "if")?
                        .is_some()
                        && p1
                            .header
                            .str_optional(doc.name, p1.line_number, "$processor$")?
                            .is_some())
                );
                if let Some(expr) = p1.header.str_optional(doc.name, p1.line_number, "if")? {
                    let val = v.get_value(p1, &doc)?;
                    v.conditions.push((
                        ftd::p2::Boolean::from_expression(
                            expr,
                            &doc,
                            &Default::default(),
                            (None, None),
                            p1.line_number,
                        )?,
                        val,
                    ));
                } else if p1
                    .header
                    .str_optional(doc.name, p1.line_number, "$processor$")?
                    .is_some()
                {
                    let start = std::time::Instant::now();
                    let value = self.lib.process(p1, &doc)?;
                    *d_processor = d_processor.saturating_add(std::time::Instant::now() - start);
                    v.value = ftd::PropertyValue::Value { value };
                } else {
                    v.update_from_p1(p1, &doc)?;
                }
                thing.push((
                    doc.resolve_name(p1.line_number, &p1.name.to_string())?,
                    ftd::p2::Thing::Variable(v),
                ));
            } else {
                // cloning because https://github.com/rust-lang/rust/issues/59159
                match (doc.get_thing(p1.line_number, p1.name.as_str())?).clone() {
                    ftd::p2::Thing::Variable(_) => {
                        return ftd::e2(
                            format!("variable should have prefix $, found: `{}`", p1.name),
                            doc.name,
                            p1.line_number,
                        );
                    }
                    ftd::p2::Thing::Component(c) => {
                        let p1 = {
                            let mut p1 = p1.clone();
                            if p1
                                .header
                                .str_optional(doc.name, p1.line_number, "$processor$")?
                                .is_some()
                            {
                                let value = self.lib.process(&p1, &doc)?;
                                Self::p1_from_processor(&mut p1, value);
                            }
                            p1
                        };
                        if let Ok(loop_data) = p1.header.str(doc.name, p1.line_number, "$loop$") {
                            let section_to_subsection = ftd::p1::SubSection {
                                name: p1.name.to_string(),
                                caption: p1.caption.to_owned(),
                                header: p1.header.to_owned(),
                                body: p1.body.to_owned(),
                                is_commented: p1.is_commented,
                                line_number: p1.line_number,
                            };
                            instructions.push(ftd::Instruction::RecursiveChildComponent {
                                child: ftd::component::recursive_child_component(
                                    loop_data,
                                    &section_to_subsection,
                                    &doc,
                                    &Default::default(),
                                    None,
                                )?,
                            });
                        } else {
                            let parent = ftd::ChildComponent::from_p1(
                                p1.line_number,
                                p1.name.as_str(),
                                &p1.header,
                                &p1.caption,
                                &p1.body_without_comment(),
                                &doc,
                                &c.arguments,
                            )?;

                            let mut children = vec![];

                            for sub in p1.sub_sections.0.iter() {
                                if sub.is_commented {
                                    continue;
                                }
                                if let Ok(loop_data) =
                                    sub.header.str(doc.name, p1.line_number, "$loop$")
                                {
                                    children.push(ftd::component::recursive_child_component(
                                        loop_data,
                                        sub,
                                        &doc,
                                        &parent.arguments,
                                        None,
                                    )?);
                                } else {
                                    let root_name = ftd::p2::utils::get_root_component_name(
                                        &doc,
                                        parent.root.as_str(),
                                        sub.line_number,
                                    )?;
                                    let child = if root_name.eq("ftd#text") {
                                        ftd::p2::utils::get_markup_child(
                                            sub,
                                            &doc,
                                            &parent.arguments,
                                        )?
                                    } else {
                                        ftd::ChildComponent::from_p1(
                                            sub.line_number,
                                            sub.name.as_str(),
                                            &sub.header,
                                            &sub.caption,
                                            &sub.body_without_comment(),
                                            &doc,
                                            &parent.arguments,
                                        )?
                                    };
                                    children.push(child);
                                }
                            }

                            instructions.push(ftd::Instruction::Component { children, parent })
                        }
                    }
                    ftd::p2::Thing::Record(mut r) => {
                        r.add_instance(p1, &doc)?;
                        thing.push((
                            doc.resolve_name(p1.line_number, &p1.name.to_string())?,
                            ftd::p2::Thing::Record(r),
                        ));
                    }
                    ftd::p2::Thing::OrType(_r) => {
                        // do we allow initialization of a record by name? nopes
                        return ftd::e2(
                            format!("'{}' is an or-type", p1.name.as_str()),
                            doc.name,
                            p1.line_number,
                        );
                    }
                    ftd::p2::Thing::OrTypeWithVariant { .. } => {
                        // do we allow initialization of a record by name? nopes
                        return ftd::e2(
                            format!("'{}' is an or-type variant", p1.name.as_str(),),
                            doc.name,
                            p1.line_number,
                        );
                    }
                };
            }
            self.bag.extend(thing);
        }

        if is_main {
            self.p1 = p1;
            self.aliases = aliases;
        }
        Ok(instructions)
    }

    pub(crate) fn new(lib: &'a dyn ftd::p2::Library) -> Self {
        Self {
            lib,
            bag: default_bag(),
            p1: Default::default(),
            aliases: Default::default(),
            parsed_libs: Default::default(),
        }
    }

    pub(crate) fn p1_from_processor(p1: &mut ftd::p1::Section, value: ftd::Value) {
        if let ftd::Value::Object { values } = value {
            for (k, v) in values {
                let v = if let ftd::PropertyValue::Value { value } = v {
                    if let Some(v) = value.to_string() {
                        v
                    } else {
                        continue;
                    }
                } else {
                    continue;
                };

                if k.eq("$body$") {
                    p1.body = Some((p1.line_number, v));
                } else if k.eq("$caption$") {
                    p1.caption = Some(v);
                } else {
                    p1.header.0.push((p1.line_number, k, v));
                }
            }
        }
    }
}

pub fn interpret(
    name: &str,
    source: &str,
    lib: &dyn ftd::p2::Library,
) -> ftd::p1::Result<(
    std::collections::BTreeMap<String, ftd::p2::Thing>,
    ftd::Column,
)> {
    let mut interpreter = Interpreter::new(lib);
    let instructions = interpreter.interpret(name, source)?;
    dbg!(&instructions);
    let mut rt = ftd::RT::from(name, interpreter.aliases, interpreter.bag, instructions);
    let main = rt.render_()?;
    Ok((rt.bag, main))
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, PartialEq, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum Thing {
    Component(ftd::Component),
    Variable(ftd::Variable),
    Record(ftd::p2::Record),
    OrType(ftd::OrType),
    OrTypeWithVariant { e: ftd::OrType, variant: String },
    // Library -> Name of library successfully parsed
}

pub fn default_bag() -> std::collections::BTreeMap<String, ftd::p2::Thing> {
    std::array::IntoIter::new([
        (
            "ftd#row".to_string(),
            ftd::p2::Thing::Component(ftd::p2::element::row_function()),
        ),
        (
            "ftd#column".to_string(),
            ftd::p2::Thing::Component(ftd::p2::element::column_function()),
        ),
        /*(
            "ftd#text".to_string(),
            ftd::p2::Thing::Component(ftd::p2::element::text_function(false)),
        ),*/
        (
            "ftd#text-block".to_string(),
            ftd::p2::Thing::Component(ftd::p2::element::text_function()),
        ),
        (
            "ftd#code".to_string(),
            ftd::p2::Thing::Component(ftd::p2::element::code_function()),
        ),
        (
            "ftd#image".to_string(),
            ftd::p2::Thing::Component(ftd::p2::element::image_function()),
        ),
        (
            "ftd#iframe".to_string(),
            ftd::p2::Thing::Component(ftd::p2::element::iframe_function()),
        ),
        (
            "ftd#integer".to_string(),
            ftd::p2::Thing::Component(ftd::p2::element::integer_function()),
        ),
        (
            "ftd#decimal".to_string(),
            ftd::p2::Thing::Component(ftd::p2::element::decimal_function()),
        ),
        (
            "ftd#boolean".to_string(),
            ftd::p2::Thing::Component(ftd::p2::element::boolean_function()),
        ),
        (
            "ftd#scene".to_string(),
            ftd::p2::Thing::Component(ftd::p2::element::scene_function()),
        ),
        (
            "ftd#grid".to_string(),
            ftd::p2::Thing::Component(ftd::p2::element::grid_function()),
        ),
        (
            "ftd#text".to_string(),
            ftd::p2::Thing::Component(ftd::p2::element::markup_function()),
        ),
        (
            "ftd#input".to_string(),
            ftd::p2::Thing::Component(ftd::p2::element::input_function()),
        ),
        (
            "ftd#null".to_string(),
            ftd::p2::Thing::Component(ftd::p2::element::null()),
        ),
    ])
    .collect()
}

pub fn default_aliases() -> std::collections::BTreeMap<String, String> {
    std::array::IntoIter::new([("ftd".to_string(), "ftd".to_string())]).collect()
}

pub fn default_column() -> ftd::Column {
    ftd::Column {
        common: ftd::Common {
            width: Some(ftd::Length::Fill),
            height: Some(ftd::Length::Fill),
            position: Some(ftd::Position::Center),
            ..Default::default()
        },
        spacing: None,
        ..Default::default()
    }
}

// #[cfg(test)]
// pub fn elapsed(e: std::time::Duration) -> String {
//     // NOTE: there is a copy of this function in ftd also
//     let nanos = e.subsec_nanos();
//     let fraction = match nanos {
//         t if nanos < 1000 => format!("{}ns", t),
//         t if nanos < 1_000_000 => format!("{:.*}µs", 3, f64::from(t) / 1000.0),
//         t => format!("{:.*}ms", 3, f64::from(t) / 1_000_000.0),
//     };
//     let secs = e.as_secs();
//     match secs {
//         _ if secs == 0 => fraction,
//         t if secs < 5 => format!("{}.{:06}s", t, nanos / 1000),
//         t if secs < 60 => format!("{}.{:03}s", t, nanos / 1_000_000),
//         t if secs < 3600 => format!("{}m {}s", t / 60, t % 60),
//         t if secs < 86400 => format!("{}h {}m", t / 3600, (t % 3600) / 60),
//         t => format!("{}s", t),
//     }
// }

#[cfg(test)]
mod test {
    use ftd::test::*;
    use ftd::{markdown_line, Instruction};

    #[test]
    fn basic_1() {
        let mut bag = super::default_bag();
        bag.insert(
            "foo/bar#foo".to_string(),
            ftd::p2::Thing::Component(ftd::Component {
                root: "ftd.text".to_string(),
                full_name: s("foo/bar#foo"),
                properties: std::array::IntoIter::new([(
                    s("text"),
                    ftd::component::Property {
                        default: Some(ftd::PropertyValue::Value {
                            value: ftd::Value::String {
                                text: s("hello"),
                                source: ftd::TextSource::Header,
                            },
                        }),
                        conditions: vec![],
                        ..Default::default()
                    },
                )])
                .collect(),
                ..Default::default()
            }),
        );
        bag.insert(
            "foo/bar#x".to_string(),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: "x".to_string(),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::Integer { value: 10 },
                },
                conditions: vec![],
            }),
        );

        p!(
            "
            -- ftd.text foo:
            text: hello

            -- integer x: 10
            ",
            (bag, super::default_column()),
        );
    }

    #[test]
    fn conditional_attribute() {
        let mut bag = super::default_bag();
        bag.insert(
            "foo/bar#foo".to_string(),
            ftd::p2::Thing::Component(ftd::Component {
                full_name: s("foo/bar#foo"),
                root: "ftd.text".to_string(),
                arguments: std::array::IntoIter::new([(s("name"), ftd::p2::Kind::caption())])
                    .collect(),
                properties: std::array::IntoIter::new([
                    (
                        s("color"),
                        ftd::component::Property {
                            default: Some(ftd::PropertyValue::Value {
                                value: ftd::Value::String {
                                    text: "white".to_string(),
                                    source: ftd::TextSource::Header,
                                },
                            }),
                            conditions: vec![
                                (
                                    ftd::p2::Boolean::Equal {
                                        left: ftd::PropertyValue::Reference {
                                            name: "foo/bar#present".to_string(),
                                            kind: ftd::p2::Kind::boolean(),
                                        },
                                        right: ftd::PropertyValue::Value {
                                            value: ftd::Value::Boolean { value: true },
                                        },
                                    },
                                    ftd::PropertyValue::Value {
                                        value: ftd::Value::String {
                                            text: "green".to_string(),
                                            source: ftd::TextSource::Header,
                                        },
                                    },
                                ),
                                (
                                    ftd::p2::Boolean::Equal {
                                        left: ftd::PropertyValue::Reference {
                                            name: "foo/bar#present".to_string(),
                                            kind: ftd::p2::Kind::boolean(),
                                        },
                                        right: ftd::PropertyValue::Value {
                                            value: ftd::Value::Boolean { value: false },
                                        },
                                    },
                                    ftd::PropertyValue::Value {
                                        value: ftd::Value::String {
                                            text: "red".to_string(),
                                            source: ftd::TextSource::Header,
                                        },
                                    },
                                ),
                            ],
                            ..Default::default()
                        },
                    ),
                    (
                        s("text"),
                        ftd::component::Property {
                            default: Some(ftd::PropertyValue::Variable {
                                name: "name".to_string(),
                                kind: ftd::p2::Kind::caption_or_body(),
                            }),
                            conditions: vec![],
                            ..Default::default()
                        },
                    ),
                ])
                .collect(),
                ..Default::default()
            }),
        );

        bag.insert(
            "foo/bar#present".to_string(),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: "present".to_string(),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::Boolean { value: false },
                },
                conditions: vec![],
            }),
        );

        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("hello"),
                line: true,
                common: ftd::Common {
                    color: Some(ftd::Color {
                        r: 255,
                        g: 0,
                        b: 0,
                        alpha: 1.0,
                    }),
                    conditional_attribute: std::array::IntoIter::new([(
                        s("color"),
                        ftd::ConditionalAttribute {
                            attribute_type: ftd::AttributeType::Style,
                            conditions_with_value: vec![
                                (
                                    ftd::Condition {
                                        variable: s("foo/bar#present"),
                                        value: s("true"),
                                    },
                                    ftd::ConditionalValue {
                                        value: s("rgba(0,128,0,1)"),
                                        important: false,
                                    },
                                ),
                                (
                                    ftd::Condition {
                                        variable: s("foo/bar#present"),
                                        value: s("false"),
                                    },
                                    ftd::ConditionalValue {
                                        value: s("rgba(255,0,0,1)"),
                                        important: false,
                                    },
                                ),
                            ],
                            default: Some(ftd::ConditionalValue {
                                value: s("rgba(255,255,255,1)"),
                                important: false,
                            }),
                        },
                    )])
                    .collect(),
                    locals: std::array::IntoIter::new([(s("name@0"), s("hello"))]).collect(),
                    reference: Some(s("@name@0")),
                    ..Default::default()
                },
                ..Default::default()
            }));

        p!(
            "
            -- boolean present: false

            -- ftd.text foo:
            caption name:
            color: white
            color if $present: green
            color if not $present: red
            text: $name

            -- foo: hello
            ",
            (bag, main),
        );
    }

    #[test]
    fn creating_a_tree() {
        let mut bag = super::default_bag();

        bag.insert(
            "foo/bar#ft_toc".to_string(),
            ftd::p2::Thing::Component(ftd::Component {
                root: "ftd.column".to_string(),
                full_name: "foo/bar#ft_toc".to_string(),
                arguments: Default::default(),
                properties: Default::default(),
                instructions: vec![
                    ftd::component::Instruction::ChildComponent {
                        child: ftd::component::ChildComponent {
                            events: vec![],
                            root: "foo/bar#table-of-content".to_string(),
                            condition: None,
                            properties: std::array::IntoIter::new([(
                                s("id"),
                                ftd::component::Property {
                                    default: Some(ftd::PropertyValue::Value {
                                        value: ftd::variable::Value::String {
                                            text: "toc_main".to_string(),
                                            source: ftd::TextSource::Header,
                                        },
                                    }),
                                    conditions: vec![],
                                    ..Default::default()
                                },
                            )])
                            .collect(),
                            arguments: Default::default(),
                            is_recursive: false,
                            ..Default::default()
                        },
                    },
                    ftd::component::Instruction::ChildComponent {
                        child: ftd::component::ChildComponent {
                            is_recursive: false,
                            events: vec![],
                            root: "foo/bar#parent".to_string(),
                            condition: None,
                            properties: std::array::IntoIter::new([
                                (
                                    s("active"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::variable::Value::Boolean { value: true },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                                (
                                    s("id"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::variable::Value::String {
                                                text: "/welcome/".to_string(),
                                                source: ftd::TextSource::Header,
                                            },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                                (
                                    s("name"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::variable::Value::String {
                                                text: "5PM Tasks".to_string(),
                                                source: ftd::TextSource::Header,
                                            },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                            ])
                            .collect(),
                            ..Default::default()
                        },
                    },
                    ftd::component::Instruction::ChildComponent {
                        child: ftd::component::ChildComponent {
                            is_recursive: false,
                            events: vec![],
                            root: "foo/bar#parent".to_string(),
                            condition: None,
                            properties: std::array::IntoIter::new([
                                (
                                    s("id"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::variable::Value::String {
                                                text: "/Building/".to_string(),
                                                source: ftd::TextSource::Header,
                                            },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                                (
                                    s("name"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::variable::Value::String {
                                                text: "Log".to_string(),
                                                source: ftd::TextSource::Header,
                                            },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                            ])
                            .collect(),
                            ..Default::default()
                        },
                    },
                    ftd::component::Instruction::ChildComponent {
                        child: ftd::component::ChildComponent {
                            is_recursive: false,
                            events: vec![],
                            root: "foo/bar#parent".to_string(),
                            condition: None,
                            properties: std::array::IntoIter::new([
                                (
                                    s("id"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::variable::Value::String {
                                                text: "/ChildBuilding/".to_string(),
                                                source: ftd::TextSource::Header,
                                            },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                                (
                                    s("name"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::variable::Value::String {
                                                text: "ChildLog".to_string(),
                                                source: ftd::TextSource::Header,
                                            },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                            ])
                            .collect(),
                            ..Default::default()
                        },
                    },
                    ftd::component::Instruction::ChangeContainer {
                        name: "/welcome/".to_string(),
                    },
                    ftd::component::Instruction::ChildComponent {
                        child: ftd::component::ChildComponent {
                            is_recursive: false,
                            events: vec![],
                            root: "foo/bar#parent".to_string(),
                            condition: None,
                            properties: std::array::IntoIter::new([
                                (
                                    s("id"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::variable::Value::String {
                                                text: "/Building2/".to_string(),
                                                source: ftd::TextSource::Header,
                                            },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                                (
                                    s("name"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::variable::Value::String {
                                                text: "Log2".to_string(),
                                                source: ftd::TextSource::Header,
                                            },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                            ])
                            .collect(),
                            ..Default::default()
                        },
                    },
                ],
                kernel: false,
                ..Default::default()
            }),
        );

        bag.insert(
            "foo/bar#parent".to_string(),
            ftd::p2::Thing::Component(ftd::Component {
                root: "ftd.column".to_string(),
                full_name: "foo/bar#parent".to_string(),
                arguments: std::array::IntoIter::new([
                    (
                        s("active"),
                        ftd::p2::Kind::Optional {
                            kind: Box::new(ftd::p2::Kind::boolean()),
                        },
                    ),
                    (s("id"), ftd::p2::Kind::string()),
                    (s("name"), ftd::p2::Kind::caption()),
                ])
                .collect(),
                properties: std::array::IntoIter::new([
                    (
                        s("id"),
                        ftd::component::Property {
                            default: Some(ftd::PropertyValue::Variable {
                                name: "id".to_string(),
                                kind: ftd::p2::Kind::Optional {
                                    kind: Box::new(ftd::p2::Kind::string()),
                                },
                            }),
                            conditions: vec![],
                            ..Default::default()
                        },
                    ),
                    (
                        s("open"),
                        ftd::component::Property {
                            default: Some(ftd::PropertyValue::Value {
                                value: ftd::variable::Value::String {
                                    text: "true".to_string(),
                                    source: ftd::TextSource::Header,
                                },
                            }),
                            conditions: vec![],
                            ..Default::default()
                        },
                    ),
                    (
                        s("width"),
                        ftd::component::Property {
                            default: Some(ftd::PropertyValue::Value {
                                value: ftd::variable::Value::String {
                                    text: "fill".to_string(),
                                    source: ftd::TextSource::Header,
                                },
                            }),
                            conditions: vec![],
                            ..Default::default()
                        },
                    ),
                ])
                .collect(),
                instructions: vec![
                    ftd::component::Instruction::ChildComponent {
                        child: ftd::component::ChildComponent {
                            is_recursive: false,
                            events: vec![],
                            root: "ftd#text".to_string(),
                            condition: Some(ftd::p2::Boolean::IsNotNull {
                                value: ftd::PropertyValue::Variable {
                                    name: "active".to_string(),
                                    kind: ftd::p2::Kind::Optional {
                                        kind: Box::new(ftd::p2::Kind::boolean()),
                                    },
                                },
                            }),
                            properties: std::array::IntoIter::new([
                                (
                                    s("color"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::variable::Value::String {
                                                text: "white".to_string(),
                                                source: ftd::TextSource::Header,
                                            },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                                (
                                    s("size"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::variable::Value::Integer { value: 14 },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                                (
                                    s("text"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Variable {
                                            name: "name".to_string(),
                                            kind: ftd::p2::Kind::caption_or_body(),
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                            ])
                            .collect(),
                            ..Default::default()
                        },
                    },
                    ftd::component::Instruction::ChildComponent {
                        child: ftd::component::ChildComponent {
                            is_recursive: false,
                            events: vec![],
                            root: "ftd#text".to_string(),
                            condition: Some(ftd::p2::Boolean::IsNull {
                                value: ftd::PropertyValue::Variable {
                                    name: "active".to_string(),
                                    kind: ftd::p2::Kind::Optional {
                                        kind: Box::new(ftd::p2::Kind::boolean()),
                                    },
                                },
                            }),
                            properties: std::array::IntoIter::new([
                                (
                                    s("color"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::variable::Value::String {
                                                text: "#4D4D4D".to_string(),
                                                source: ftd::TextSource::Header,
                                            },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                                (
                                    s("size"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::variable::Value::Integer { value: 14 },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                                (
                                    s("text"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Variable {
                                            name: "name".to_string(),
                                            kind: ftd::p2::Kind::caption_or_body(),
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                            ])
                            .collect(),
                            ..Default::default()
                        },
                    },
                ],
                kernel: false,
                ..Default::default()
            }),
        );

        bag.insert(
            "foo/bar#table-of-content".to_string(),
            ftd::p2::Thing::Component(ftd::Component {
                root: "ftd.column".to_string(),
                full_name: "foo/bar#table-of-content".to_string(),
                arguments: std::array::IntoIter::new([(s("id"), ftd::p2::Kind::string())])
                    .collect(),
                properties: std::array::IntoIter::new([
                    (
                        s("height"),
                        ftd::component::Property {
                            default: Some(ftd::PropertyValue::Value {
                                value: ftd::variable::Value::String {
                                    text: "fill".to_string(),
                                    source: ftd::TextSource::Header,
                                },
                            }),
                            conditions: vec![],
                            ..Default::default()
                        },
                    ),
                    (
                        s("id"),
                        ftd::component::Property {
                            default: Some(ftd::PropertyValue::Variable {
                                name: "id".to_string(),
                                kind: ftd::p2::Kind::Optional {
                                    kind: Box::new(ftd::p2::Kind::string()),
                                },
                            }),
                            conditions: vec![],
                            ..Default::default()
                        },
                    ),
                    (
                        s("width"),
                        ftd::component::Property {
                            default: Some(ftd::PropertyValue::Value {
                                value: ftd::variable::Value::String {
                                    text: "300".to_string(),
                                    source: ftd::TextSource::Header,
                                },
                            }),
                            conditions: vec![],
                            ..Default::default()
                        },
                    ),
                ])
                .collect(),
                instructions: vec![],
                kernel: false,
                ..Default::default()
            }),
        );

        bag.insert(
            "foo/bar#toc-heading".to_string(),
            ftd::p2::Thing::Component(ftd::Component {
                root: "ftd.text".to_string(),
                full_name: "foo/bar#toc-heading".to_string(),
                arguments: std::array::IntoIter::new([(s("text"), ftd::p2::Kind::caption())])
                    .collect(),
                properties: std::array::IntoIter::new([
                    (
                        s("size"),
                        ftd::component::Property {
                            default: Some(ftd::PropertyValue::Value {
                                value: ftd::variable::Value::Integer { value: 16 },
                            }),
                            conditions: vec![],
                            ..Default::default()
                        },
                    ),
                    (
                        s("text"),
                        ftd::component::Property {
                            default: Some(ftd::PropertyValue::Variable {
                                name: "text".to_string(),
                                kind: ftd::p2::Kind::caption_or_body(),
                            }),
                            conditions: vec![],
                            ..Default::default()
                        },
                    ),
                ])
                .collect(),
                ..Default::default()
            }),
        );

        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![ftd::Element::Column(ftd::Column {
                        spacing: None,
                        container: ftd::Container {
                            children: vec![ftd::Element::Column(ftd::Column {
                                spacing: None,
                                container: ftd::Container {
                                    children: vec![
                                        ftd::Element::Markup(ftd::Markups {
                                            text: ftd::markdown_line("5PM Tasks"),
                                            line: true,
                                            common: ftd::Common {
                                                color: Some(ftd::Color {
                                                    r: 255,
                                                    g: 255,
                                                    b: 255,
                                                    alpha: 1.0,
                                                }),
                                                reference: Some(s("@name@0,0,0")),
                                                condition: Some(ftd::Condition {
                                                    variable: s("@active@0,0,0"),
                                                    value: s("$IsNotNull$"),
                                                }),
                                                ..Default::default()
                                            },
                                            size: Some(14),
                                            ..Default::default()
                                        }),
                                        ftd::Element::Markup(ftd::Markups {
                                            text: ftd::markdown_line("5PM Tasks"),
                                            line: true,
                                            common: ftd::Common {
                                                color: Some(ftd::Color {
                                                    r: 77,
                                                    g: 77,
                                                    b: 77,
                                                    alpha: 1.0,
                                                }),
                                                reference: Some(s("@name@0,0,0")),
                                                condition: Some(ftd::Condition {
                                                    variable: s("@active@0,0,0"),
                                                    value: s("$IsNull$"),
                                                }),
                                                is_not_visible: true,
                                                ..Default::default()
                                            },
                                            size: Some(14),
                                            ..Default::default()
                                        }),
                                        ftd::Element::Column(ftd::Column {
                                            spacing: None,
                                            container: ftd::Container {
                                                children: vec![
                                                    ftd::Element::Markup(ftd::Markups {
                                                        text: ftd::markdown_line("Log"),
                                                        line: true,
                                                        common: ftd::Common {
                                                            color: Some(ftd::Color {
                                                                r: 255,
                                                                g: 255,
                                                                b: 255,
                                                                alpha: 1.0,
                                                            }),
                                                            reference: Some(s("@name@0,0,0,0")),
                                                            condition: Some(ftd::Condition {
                                                                variable: s("@active@0,0,0,0"),
                                                                value: s("$IsNotNull$"),
                                                            }),
                                                            is_not_visible: true,
                                                            ..Default::default()
                                                        },
                                                        size: Some(14),
                                                        ..Default::default()
                                                    }),
                                                    ftd::Element::Markup(ftd::Markups {
                                                        text: ftd::markdown_line("Log"),
                                                        line: true,
                                                        common: ftd::Common {
                                                            color: Some(ftd::Color {
                                                                r: 77,
                                                                g: 77,
                                                                b: 77,
                                                                alpha: 1.0,
                                                            }),
                                                            reference: Some(s("@name@0,0,0,0")),
                                                            condition: Some(ftd::Condition {
                                                                variable: s("@active@0,0,0,0"),
                                                                value: s("$IsNull$"),
                                                            }),
                                                            ..Default::default()
                                                        },
                                                        size: Some(14),
                                                        ..Default::default()
                                                    }),
                                                    ftd::Element::Column(ftd::Column {
                                                        spacing: None,
                                                        container: ftd::Container {
                                                            external_children: Default::default(),
                                                            children: vec![
                                                            ftd::Element::Markup(ftd::Markups {
                                                                text: ftd::markdown_line(
                                                                    "ChildLog",
                                                                ),
                                                                line: true,
                                                                common: ftd::Common {
                                                                    color: Some(ftd::Color {
                                                                        r: 255,
                                                                        g: 255,
                                                                        b: 255,
                                                                        alpha: 1.0,
                                                                    }),
                                                                    reference: Some(s(
                                                                        "@name@0,0,0,0,0",
                                                                    )),
                                                                    condition: Some(
                                                                        ftd::Condition {
                                                                            variable: s(
                                                                                "@active@0,0,0,0,0",
                                                                            ),
                                                                            value: s("$IsNotNull$"),
                                                                        },
                                                                    ),
                                                                    is_not_visible: true,
                                                                    ..Default::default()
                                                                },
                                                                size: Some(14),
                                                                ..Default::default()
                                                            }),
                                                            ftd::Element::Markup(ftd::Markups {
                                                                text: ftd::markdown_line(
                                                                    "ChildLog",
                                                                ),
                                                                line: true,
                                                                common: ftd::Common {
                                                                    color: Some(ftd::Color {
                                                                        r: 77,
                                                                        g: 77,
                                                                        b: 77,
                                                                        alpha: 1.0,
                                                                    }),
                                                                    reference: Some(s(
                                                                        "@name@0,0,0,0,0",
                                                                    )),
                                                                    condition: Some(
                                                                        ftd::Condition {
                                                                            variable: s(
                                                                                "@active@0,0,0,0,0",
                                                                            ),
                                                                            value: s("$IsNull$"),
                                                                        },
                                                                    ),
                                                                    ..Default::default()
                                                                },
                                                                size: Some(14),
                                                                ..Default::default()
                                                            }),
                                                        ],
                                                            open: (Some(true), None),
                                                            ..Default::default()
                                                        },
                                                        common: ftd::Common {
                                                            locals: std::array::IntoIter::new([
                                                                (s("active@0,0,0,0,0"), s("")),
                                                                (
                                                                    s("id@0,0,0,0,0"),
                                                                    s("/ChildBuilding/"),
                                                                ),
                                                                (
                                                                    s("name@0,0,0,0,0"),
                                                                    s("ChildLog"),
                                                                ),
                                                            ])
                                                            .collect(),
                                                            data_id: Some(s("/ChildBuilding/")),
                                                            width: Some(ftd::Length::Fill),
                                                            ..Default::default()
                                                        },
                                                    }),
                                                ],
                                                external_children: Default::default(),
                                                open: (Some(true), None),
                                                ..Default::default()
                                            },
                                            common: ftd::Common {
                                                locals: std::array::IntoIter::new([
                                                    (s("active@0,0,0,0"), s("")),
                                                    (s("id@0,0,0,0"), s("/Building/")),
                                                    (s("name@0,0,0,0"), s("Log")),
                                                ])
                                                .collect(),
                                                data_id: Some(s("/Building/")),
                                                width: Some(ftd::Length::Fill),
                                                ..Default::default()
                                            },
                                        }),
                                        ftd::Element::Column(ftd::Column {
                                            spacing: None,
                                            container: ftd::Container {
                                                external_children: Default::default(),
                                                children: vec![
                                                    ftd::Element::Markup(ftd::Markups {
                                                        text: ftd::markdown_line("Log2"),
                                                        line: true,
                                                        common: ftd::Common {
                                                            color: Some(ftd::Color {
                                                                r: 255,
                                                                g: 255,
                                                                b: 255,
                                                                alpha: 1.0,
                                                            }),
                                                            reference: Some(s("@name@0,0,0,1")),
                                                            condition: Some(ftd::Condition {
                                                                variable: s("@active@0,0,0,1"),
                                                                value: s("$IsNotNull$"),
                                                            }),
                                                            is_not_visible: true,
                                                            ..Default::default()
                                                        },
                                                        size: Some(14),
                                                        ..Default::default()
                                                    }),
                                                    ftd::Element::Markup(ftd::Markups {
                                                        text: ftd::markdown_line("Log2"),
                                                        line: true,
                                                        common: ftd::Common {
                                                            color: Some(ftd::Color {
                                                                r: 77,
                                                                g: 77,
                                                                b: 77,
                                                                alpha: 1.0,
                                                            }),
                                                            reference: Some(s("@name@0,0,0,1")),
                                                            condition: Some(ftd::Condition {
                                                                variable: s("@active@0,0,0,1"),
                                                                value: s("$IsNull$"),
                                                            }),
                                                            ..Default::default()
                                                        },
                                                        size: Some(14),
                                                        ..Default::default()
                                                    }),
                                                ],
                                                open: (Some(true), None),
                                                ..Default::default()
                                            },
                                            common: ftd::Common {
                                                locals: std::array::IntoIter::new([
                                                    (s("active@0,0,0,1"), s("")),
                                                    (s("id@0,0,0,1"), s("/Building2/")),
                                                    (s("name@0,0,0,1"), s("Log2")),
                                                ])
                                                .collect(),
                                                data_id: Some(s("/Building2/")),
                                                width: Some(ftd::Length::Fill),
                                                ..Default::default()
                                            },
                                        }),
                                    ],
                                    external_children: Default::default(),
                                    open: (Some(true), None),
                                    ..Default::default()
                                },
                                common: ftd::Common {
                                    locals: std::array::IntoIter::new([
                                        (s("active@0,0,0"), s("true")),
                                        (s("id@0,0,0"), s("/welcome/")),
                                        (s("name@0,0,0"), s("5PM Tasks")),
                                    ])
                                    .collect(),
                                    data_id: Some(s("/welcome/")),
                                    width: Some(ftd::Length::Fill),
                                    ..Default::default()
                                },
                            })],
                            ..Default::default()
                        },
                        common: ftd::Common {
                            locals: std::array::IntoIter::new([(s("id@0,0"), s("toc_main"))])
                                .collect(),
                            data_id: Some(s("toc_main")),
                            height: Some(ftd::Length::Fill),
                            width: Some(ftd::Length::Px { value: 300 }),
                            ..Default::default()
                        },
                    })],
                    ..Default::default()
                },
                ..Default::default()
            }));

        p!(
            r"
            -- ftd.text toc-heading:
            caption text:
            text: $text
            size: 16


            -- ftd.column table-of-content:
            string id:
            id: $id
            width: 300
            height: fill


            -- ftd.column parent:
            string id:
            caption name:
            optional boolean active:
            id: $id
            width: fill
            open: true

            --- ftd.text:
            if: $active is not null
            text: $name
            size: 14
            color: white

            --- ftd.text:
            if: $active is null
            text: $name
            size: 14
            color: \#4D4D4D


            -- ftd.column ft_toc:

            --- table-of-content:
            id: toc_main

            --- parent:
            id: /welcome/
            name: 5PM Tasks
            active: true

            --- parent:
            id: /Building/
            name: Log

            --- parent:
            id: /ChildBuilding/
            name: ChildLog

            --- container: /welcome/

            --- parent:
            id: /Building2/
            name: Log2


            -- ft_toc:
            ",
            (bag, main),
        );
    }

    #[test]
    fn creating_a_tree_using_import() {
        let mut bag = super::default_bag();

        bag.insert(
            "creating-a-tree#ft_toc".to_string(),
            ftd::p2::Thing::Component(ftd::Component {
                root: "ftd.column".to_string(),
                full_name: "creating-a-tree#ft_toc".to_string(),
                arguments: Default::default(),
                properties: Default::default(),
                instructions: vec![
                    ftd::component::Instruction::ChildComponent {
                        child: ftd::component::ChildComponent {
                            is_recursive: false,
                            events: vec![],
                            root: "creating-a-tree#table-of-content".to_string(),
                            condition: None,
                            properties: std::array::IntoIter::new([(
                                s("id"),
                                ftd::component::Property {
                                    default: Some(ftd::PropertyValue::Value {
                                        value: ftd::variable::Value::String {
                                            text: "toc_main".to_string(),
                                            source: ftd::TextSource::Header,
                                        },
                                    }),
                                    conditions: vec![],
                                    ..Default::default()
                                },
                            )])
                            .collect(),
                            ..Default::default()
                        },
                    },
                    ftd::component::Instruction::ChildComponent {
                        child: ftd::component::ChildComponent {
                            is_recursive: false,
                            events: vec![],
                            root: "creating-a-tree#parent".to_string(),
                            condition: None,
                            properties: std::array::IntoIter::new([
                                (
                                    s("active"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::variable::Value::Boolean { value: true },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                                (
                                    s("id"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::variable::Value::String {
                                                text: "/welcome/".to_string(),
                                                source: ftd::TextSource::Header,
                                            },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                                (
                                    s("name"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::variable::Value::String {
                                                text: "5PM Tasks".to_string(),
                                                source: ftd::TextSource::Header,
                                            },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                            ])
                            .collect(),
                            ..Default::default()
                        },
                    },
                    ftd::component::Instruction::ChildComponent {
                        child: ftd::component::ChildComponent {
                            is_recursive: false,
                            events: vec![],
                            root: "creating-a-tree#parent".to_string(),
                            condition: None,
                            properties: std::array::IntoIter::new([
                                (
                                    s("id"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::variable::Value::String {
                                                text: "/Building/".to_string(),
                                                source: ftd::TextSource::Header,
                                            },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                                (
                                    s("name"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::variable::Value::String {
                                                text: "Log".to_string(),
                                                source: ftd::TextSource::Header,
                                            },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                            ])
                            .collect(),
                            ..Default::default()
                        },
                    },
                    ftd::component::Instruction::ChildComponent {
                        child: ftd::component::ChildComponent {
                            is_recursive: false,
                            events: vec![],
                            root: "creating-a-tree#parent".to_string(),
                            condition: None,
                            properties: std::array::IntoIter::new([
                                (
                                    s("id"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::variable::Value::String {
                                                text: "/ChildBuilding/".to_string(),
                                                source: ftd::TextSource::Header,
                                            },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                                (
                                    s("name"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::variable::Value::String {
                                                text: "ChildLog".to_string(),
                                                source: ftd::TextSource::Header,
                                            },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                            ])
                            .collect(),
                            ..Default::default()
                        },
                    },
                    ftd::component::Instruction::ChangeContainer {
                        name: "/welcome/".to_string(),
                    },
                    ftd::component::Instruction::ChildComponent {
                        child: ftd::component::ChildComponent {
                            is_recursive: false,
                            events: vec![],
                            root: "creating-a-tree#parent".to_string(),
                            condition: None,
                            properties: std::array::IntoIter::new([
                                (
                                    s("id"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::variable::Value::String {
                                                text: "/Building2/".to_string(),
                                                source: ftd::TextSource::Header,
                                            },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                                (
                                    s("name"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::variable::Value::String {
                                                text: "Log2".to_string(),
                                                source: ftd::TextSource::Header,
                                            },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                            ])
                            .collect(),
                            ..Default::default()
                        },
                    },
                ],
                kernel: false,
                ..Default::default()
            }),
        );

        bag.insert(
            "creating-a-tree#parent".to_string(),
            ftd::p2::Thing::Component(ftd::Component {
                root: "ftd.column".to_string(),
                full_name: "creating-a-tree#parent".to_string(),
                arguments: std::array::IntoIter::new([
                    (
                        s("active"),
                        ftd::p2::Kind::Optional {
                            kind: Box::new(ftd::p2::Kind::boolean()),
                        },
                    ),
                    (s("id"), ftd::p2::Kind::string()),
                    (s("name"), ftd::p2::Kind::caption()),
                ])
                .collect(),
                properties: std::array::IntoIter::new([
                    (
                        s("id"),
                        ftd::component::Property {
                            default: Some(ftd::PropertyValue::Variable {
                                name: "id".to_string(),
                                kind: ftd::p2::Kind::Optional {
                                    kind: Box::new(ftd::p2::Kind::string()),
                                },
                            }),
                            conditions: vec![],
                            ..Default::default()
                        },
                    ),
                    (
                        s("open"),
                        ftd::component::Property {
                            default: Some(ftd::PropertyValue::Value {
                                value: ftd::variable::Value::String {
                                    text: "true".to_string(),
                                    source: ftd::TextSource::Header,
                                },
                            }),
                            conditions: vec![],
                            ..Default::default()
                        },
                    ),
                    (
                        s("width"),
                        ftd::component::Property {
                            default: Some(ftd::PropertyValue::Value {
                                value: ftd::variable::Value::String {
                                    text: "fill".to_string(),
                                    source: ftd::TextSource::Header,
                                },
                            }),
                            conditions: vec![],
                            ..Default::default()
                        },
                    ),
                ])
                .collect(),
                instructions: vec![
                    ftd::component::Instruction::ChildComponent {
                        child: ftd::component::ChildComponent {
                            is_recursive: false,
                            events: vec![],
                            root: "ftd#text".to_string(),
                            condition: Some(ftd::p2::Boolean::IsNotNull {
                                value: ftd::PropertyValue::Variable {
                                    name: "active".to_string(),
                                    kind: ftd::p2::Kind::Optional {
                                        kind: Box::new(ftd::p2::Kind::boolean()),
                                    },
                                },
                            }),
                            properties: std::array::IntoIter::new([
                                (
                                    s("color"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::variable::Value::String {
                                                text: "white".to_string(),
                                                source: ftd::TextSource::Header,
                                            },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                                (
                                    s("size"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::variable::Value::Integer { value: 14 },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                                (
                                    s("text"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Variable {
                                            name: "name".to_string(),
                                            kind: ftd::p2::Kind::caption_or_body(),
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                            ])
                            .collect(),
                            ..Default::default()
                        },
                    },
                    ftd::component::Instruction::ChildComponent {
                        child: ftd::component::ChildComponent {
                            is_recursive: false,
                            events: vec![],
                            root: "ftd#text".to_string(),
                            condition: Some(ftd::p2::Boolean::IsNull {
                                value: ftd::PropertyValue::Variable {
                                    name: "active".to_string(),
                                    kind: ftd::p2::Kind::Optional {
                                        kind: Box::new(ftd::p2::Kind::boolean()),
                                    },
                                },
                            }),
                            properties: std::array::IntoIter::new([
                                (
                                    s("color"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::variable::Value::String {
                                                text: "#4D4D4D".to_string(),
                                                source: ftd::TextSource::Header,
                                            },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                                (
                                    s("size"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::variable::Value::Integer { value: 14 },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                                (
                                    s("text"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Variable {
                                            name: "name".to_string(),
                                            kind: ftd::p2::Kind::caption_or_body(),
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                            ])
                            .collect(),
                            ..Default::default()
                        },
                    },
                ],
                kernel: false,
                ..Default::default()
            }),
        );

        bag.insert(
            "creating-a-tree#table-of-content".to_string(),
            ftd::p2::Thing::Component(ftd::Component {
                root: "ftd.column".to_string(),
                full_name: "creating-a-tree#table-of-content".to_string(),
                arguments: std::array::IntoIter::new([(s("id"), ftd::p2::Kind::string())])
                    .collect(),
                properties: std::array::IntoIter::new([
                    (
                        s("height"),
                        ftd::component::Property {
                            default: Some(ftd::PropertyValue::Value {
                                value: ftd::variable::Value::String {
                                    text: "fill".to_string(),
                                    source: ftd::TextSource::Header,
                                },
                            }),
                            conditions: vec![],
                            ..Default::default()
                        },
                    ),
                    (
                        s("id"),
                        ftd::component::Property {
                            default: Some(ftd::PropertyValue::Variable {
                                name: "id".to_string(),
                                kind: ftd::p2::Kind::Optional {
                                    kind: Box::new(ftd::p2::Kind::string()),
                                },
                            }),
                            conditions: vec![],
                            ..Default::default()
                        },
                    ),
                    (
                        s("width"),
                        ftd::component::Property {
                            default: Some(ftd::PropertyValue::Value {
                                value: ftd::variable::Value::String {
                                    text: "300".to_string(),
                                    source: ftd::TextSource::Header,
                                },
                            }),
                            conditions: vec![],
                            ..Default::default()
                        },
                    ),
                ])
                .collect(),
                instructions: vec![],
                kernel: false,
                ..Default::default()
            }),
        );

        bag.insert(
            "creating-a-tree#toc-heading".to_string(),
            ftd::p2::Thing::Component(ftd::Component {
                root: "ftd.text".to_string(),
                full_name: "creating-a-tree#toc-heading".to_string(),
                arguments: std::array::IntoIter::new([(s("text"), ftd::p2::Kind::caption())])
                    .collect(),
                properties: std::array::IntoIter::new([
                    (
                        s("size"),
                        ftd::component::Property {
                            default: Some(ftd::PropertyValue::Value {
                                value: ftd::variable::Value::Integer { value: 16 },
                            }),
                            conditions: vec![],
                            ..Default::default()
                        },
                    ),
                    (
                        s("text"),
                        ftd::component::Property {
                            default: Some(ftd::PropertyValue::Variable {
                                name: "text".to_string(),
                                kind: ftd::p2::Kind::caption_or_body(),
                            }),
                            conditions: vec![],
                            ..Default::default()
                        },
                    ),
                ])
                .collect(),
                ..Default::default()
            }),
        );

        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![ftd::Element::Column(ftd::Column {
                        spacing: None,
                        container: ftd::Container {
                            children: vec![ftd::Element::Column(ftd::Column {
                                spacing: None,
                                container: ftd::Container {
                                    children: vec![
                                        ftd::Element::Markup(ftd::Markups {
                                            text: ftd::markdown_line("5PM Tasks"),
                                            line: true,
                                            common: ftd::Common {
                                                color: Some(ftd::Color {
                                                    r: 255,
                                                    g: 255,
                                                    b: 255,
                                                    alpha: 1.0,
                                                }),
                                                reference: Some(s("@name@0,0,0")),
                                                condition: Some(ftd::Condition {
                                                    variable: s("@active@0,0,0"),
                                                    value: s("$IsNotNull$"),
                                                }),
                                                ..Default::default()
                                            },
                                            size: Some(14),
                                            ..Default::default()
                                        }),
                                        ftd::Element::Markup(ftd::Markups {
                                            text: ftd::markdown_line("5PM Tasks"),
                                            line: true,
                                            common: ftd::Common {
                                                color: Some(ftd::Color {
                                                    r: 77,
                                                    g: 77,
                                                    b: 77,
                                                    alpha: 1.0,
                                                }),
                                                reference: Some(s("@name@0,0,0")),
                                                condition: Some(ftd::Condition {
                                                    variable: s("@active@0,0,0"),
                                                    value: s("$IsNull$"),
                                                }),
                                                is_not_visible: true,
                                                ..Default::default()
                                            },
                                            size: Some(14),
                                            ..Default::default()
                                        }),
                                        ftd::Element::Column(ftd::Column {
                                            spacing: None,
                                            container: ftd::Container {
                                                children: vec![
                                                    ftd::Element::Markup(ftd::Markups {
                                                        text: ftd::markdown_line("Log"),
                                                        line: true,
                                                        common: ftd::Common {
                                                            color: Some(ftd::Color {
                                                                r: 255,
                                                                g: 255,
                                                                b: 255,
                                                                alpha: 1.0,
                                                            }),
                                                            reference: Some(s("@name@0,0,0,0")),
                                                            condition: Some(ftd::Condition {
                                                                variable: s("@active@0,0,0,0"),
                                                                value: s("$IsNotNull$"),
                                                            }),
                                                            is_not_visible: true,
                                                            ..Default::default()
                                                        },
                                                        size: Some(14),
                                                        ..Default::default()
                                                    }),
                                                    ftd::Element::Markup(ftd::Markups {
                                                        text: ftd::markdown_line("Log"),
                                                        line: true,
                                                        common: ftd::Common {
                                                            color: Some(ftd::Color {
                                                                r: 77,
                                                                g: 77,
                                                                b: 77,
                                                                alpha: 1.0,
                                                            }),
                                                            reference: Some(s("@name@0,0,0,0")),
                                                            condition: Some(ftd::Condition {
                                                                variable: s("@active@0,0,0,0"),
                                                                value: s("$IsNull$"),
                                                            }),
                                                            ..Default::default()
                                                        },
                                                        size: Some(14),
                                                        ..Default::default()
                                                    }),
                                                    ftd::Element::Column(ftd::Column {
                                                        spacing: None,
                                                        container: ftd::Container {
                                                            external_children: Default::default(),
                                                            children: vec![
                                                            ftd::Element::Markup(ftd::Markups {
                                                                text: ftd::markdown_line(
                                                                    "ChildLog",
                                                                ),
                                                                line: true,
                                                                common: ftd::Common {
                                                                    color: Some(ftd::Color {
                                                                        r: 255,
                                                                        g: 255,
                                                                        b: 255,
                                                                        alpha: 1.0,
                                                                    }),
                                                                    reference: Some(s(
                                                                        "@name@0,0,0,0,0",
                                                                    )),
                                                                    condition: Some(
                                                                        ftd::Condition {
                                                                            variable: s(
                                                                                "@active@0,0,0,0,0",
                                                                            ),
                                                                            value: s("$IsNotNull$"),
                                                                        },
                                                                    ),
                                                                    is_not_visible: true,
                                                                    ..Default::default()
                                                                },
                                                                size: Some(14),
                                                                ..Default::default()
                                                            }),
                                                            ftd::Element::Markup(ftd::Markups {
                                                                text: ftd::markdown_line(
                                                                    "ChildLog",
                                                                ),
                                                                line: true,
                                                                common: ftd::Common {
                                                                    color: Some(ftd::Color {
                                                                        r: 77,
                                                                        g: 77,
                                                                        b: 77,
                                                                        alpha: 1.0,
                                                                    }),
                                                                    reference: Some(s(
                                                                        "@name@0,0,0,0,0",
                                                                    )),
                                                                    condition: Some(
                                                                        ftd::Condition {
                                                                            variable: s(
                                                                                "@active@0,0,0,0,0",
                                                                            ),
                                                                            value: s("$IsNull$"),
                                                                        },
                                                                    ),
                                                                    ..Default::default()
                                                                },
                                                                size: Some(14),
                                                                ..Default::default()
                                                            }),
                                                        ],
                                                            open: (Some(true), None),
                                                            ..Default::default()
                                                        },
                                                        common: ftd::Common {
                                                            locals: std::array::IntoIter::new([
                                                                (s("active@0,0,0,0,0"), s("")),
                                                                (
                                                                    s("id@0,0,0,0,0"),
                                                                    s("/ChildBuilding/"),
                                                                ),
                                                                (
                                                                    s("name@0,0,0,0,0"),
                                                                    s("ChildLog"),
                                                                ),
                                                            ])
                                                            .collect(),
                                                            data_id: Some(s("/ChildBuilding/")),
                                                            width: Some(ftd::Length::Fill),
                                                            ..Default::default()
                                                        },
                                                    }),
                                                ],
                                                external_children: Default::default(),
                                                open: (Some(true), None),
                                                ..Default::default()
                                            },
                                            common: ftd::Common {
                                                locals: std::array::IntoIter::new([
                                                    (s("active@0,0,0,0"), s("")),
                                                    (s("id@0,0,0,0"), s("/Building/")),
                                                    (s("name@0,0,0,0"), s("Log")),
                                                ])
                                                .collect(),
                                                data_id: Some(s("/Building/")),
                                                width: Some(ftd::Length::Fill),
                                                ..Default::default()
                                            },
                                        }),
                                        ftd::Element::Column(ftd::Column {
                                            spacing: None,
                                            container: ftd::Container {
                                                external_children: Default::default(),
                                                children: vec![
                                                    ftd::Element::Markup(ftd::Markups {
                                                        text: ftd::markdown_line("Log2"),
                                                        line: true,
                                                        common: ftd::Common {
                                                            color: Some(ftd::Color {
                                                                r: 255,
                                                                g: 255,
                                                                b: 255,
                                                                alpha: 1.0,
                                                            }),
                                                            reference: Some(s("@name@0,0,0,1")),
                                                            condition: Some(ftd::Condition {
                                                                variable: s("@active@0,0,0,1"),
                                                                value: s("$IsNotNull$"),
                                                            }),
                                                            is_not_visible: true,
                                                            ..Default::default()
                                                        },
                                                        size: Some(14),
                                                        ..Default::default()
                                                    }),
                                                    ftd::Element::Markup(ftd::Markups {
                                                        text: ftd::markdown_line("Log2"),
                                                        line: true,
                                                        common: ftd::Common {
                                                            color: Some(ftd::Color {
                                                                r: 77,
                                                                g: 77,
                                                                b: 77,
                                                                alpha: 1.0,
                                                            }),
                                                            reference: Some(s("@name@0,0,0,1")),
                                                            condition: Some(ftd::Condition {
                                                                variable: s("@active@0,0,0,1"),
                                                                value: s("$IsNull$"),
                                                            }),
                                                            ..Default::default()
                                                        },
                                                        size: Some(14),
                                                        ..Default::default()
                                                    }),
                                                ],
                                                open: (Some(true), None),
                                                ..Default::default()
                                            },
                                            common: ftd::Common {
                                                locals: std::array::IntoIter::new([
                                                    (s("active@0,0,0,1"), s("")),
                                                    (s("id@0,0,0,1"), s("/Building2/")),
                                                    (s("name@0,0,0,1"), s("Log2")),
                                                ])
                                                .collect(),
                                                data_id: Some(s("/Building2/")),
                                                width: Some(ftd::Length::Fill),
                                                ..Default::default()
                                            },
                                        }),
                                    ],
                                    external_children: Default::default(),
                                    open: (Some(true), None),
                                    ..Default::default()
                                },
                                common: ftd::Common {
                                    locals: std::array::IntoIter::new([
                                        (s("active@0,0,0"), s("true")),
                                        (s("id@0,0,0"), s("/welcome/")),
                                        (s("name@0,0,0"), s("5PM Tasks")),
                                    ])
                                    .collect(),
                                    data_id: Some(s("/welcome/")),
                                    width: Some(ftd::Length::Fill),
                                    ..Default::default()
                                },
                            })],
                            ..Default::default()
                        },
                        common: ftd::Common {
                            locals: std::array::IntoIter::new([(s("id@0,0"), s("toc_main"))])
                                .collect(),
                            data_id: Some(s("toc_main")),
                            height: Some(ftd::Length::Fill),
                            width: Some(ftd::Length::Px { value: 300 }),
                            ..Default::default()
                        },
                    })],
                    ..Default::default()
                },
                ..Default::default()
            }));

        p!(
            "
            -- import: creating-a-tree as ft

            -- ft.ft_toc:
            ",
            (bag, main),
        );
    }

    #[test]
    fn reference() {
        let mut bag = super::default_bag();

        bag.insert(
            "fifthtry/ft#dark-mode".to_string(),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: "dark-mode".to_string(),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::Boolean { value: true },
                },
                conditions: vec![],
            }),
        );

        bag.insert(
            "fifthtry/ft#toc".to_string(),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: "toc".to_string(),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::String {
                        text: "not set".to_string(),
                        source: ftd::TextSource::Caption,
                    },
                },
                conditions: vec![],
            }),
        );

        bag.insert(
            "fifthtry/ft#markdown".to_string(),
            ftd::p2::Thing::Component(ftd::Component {
                root: "ftd.text".to_string(),
                full_name: "fifthtry/ft#markdown".to_string(),
                arguments: std::array::IntoIter::new([(s("body"), ftd::p2::Kind::body())])
                    .collect(),
                properties: std::array::IntoIter::new([(
                    s("text"),
                    ftd::component::Property {
                        default: Some(ftd::PropertyValue::Variable {
                            name: "body".to_string(),
                            kind: ftd::p2::Kind::caption_or_body(),
                        }),
                        conditions: vec![],
                        ..Default::default()
                    },
                )])
                .collect(),
                ..Default::default()
            }),
        );

        bag.insert(
            "reference#name".to_string(),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: "name".to_string(),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::String {
                        text: "John smith".to_string(),
                        source: ftd::TextSource::Caption,
                    },
                },
                conditions: vec![],
            }),
        );

        bag.insert(
            "reference#test-component".to_string(),
            ftd::p2::Thing::Component(ftd::Component {
                root: "ftd.column".to_string(),
                full_name: "reference#test-component".to_string(),
                arguments: Default::default(),
                properties: std::array::IntoIter::new([
                    (
                        s("background-color"),
                        ftd::component::Property {
                            default: Some(ftd::PropertyValue::Value {
                                value: ftd::variable::Value::String {
                                    text: "#f3f3f3".to_string(),
                                    source: ftd::TextSource::Header,
                                },
                            }),
                            conditions: vec![],
                            ..Default::default()
                        },
                    ),
                    (
                        s("width"),
                        ftd::component::Property {
                            default: Some(ftd::PropertyValue::Value {
                                value: ftd::variable::Value::String {
                                    text: "200".to_string(),
                                    source: ftd::TextSource::Header,
                                },
                            }),
                            conditions: vec![],
                            ..Default::default()
                        },
                    ),
                ])
                .collect(),
                instructions: vec![ftd::component::Instruction::ChildComponent {
                    child: ftd::component::ChildComponent {
                        is_recursive: false,
                        events: vec![],
                        root: "ftd#text".to_string(),
                        condition: None,
                        properties: std::array::IntoIter::new([(
                            s("text"),
                            ftd::component::Property {
                                default: Some(ftd::PropertyValue::Reference {
                                    name: "reference#name".to_string(),
                                    kind: ftd::p2::Kind::caption_or_body(),
                                }),
                                conditions: vec![],
                                ..Default::default()
                            },
                        )])
                        .collect(),
                        ..Default::default()
                    },
                }],
                kernel: false,
                ..Default::default()
            }),
        );
        let title = ftd::Markups {
            text: ftd::markdown_line("John smith"),
            line: true,
            common: ftd::Common {
                reference: Some(s("reference#name")),
                ..Default::default()
            },
            ..Default::default()
        };

        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                common: ftd::Common {
                    width: Some(ftd::Length::Px { value: 200 }),
                    background_color: Some(ftd::Color {
                        r: 243,
                        g: 243,
                        b: 243,
                        alpha: 1.0,
                    }),
                    ..Default::default()
                },
                container: ftd::Container {
                    children: vec![ftd::Element::Markup(title)],
                    ..Default::default()
                },
            }));

        p!(
            "
            -- import: reference as ct

            -- ct.test-component:
            ",
            (bag, main),
        );
    }

    #[test]
    fn text() {
        let mut bag = super::default_bag();

        bag.insert(
            "foo/bar#foo".to_string(),
            ftd::p2::Thing::Component(ftd::Component {
                full_name: s("foo/bar#foo"),
                root: "ftd.text".to_string(),
                arguments: std::array::IntoIter::new([(
                    s("name"),
                    ftd::p2::Kind::caption_or_body(),
                )])
                .collect(),
                properties: std::array::IntoIter::new([(
                    s("text"),
                    ftd::component::Property {
                        default: Some(ftd::PropertyValue::Variable {
                            name: "name".to_string(),
                            kind: ftd::p2::Kind::caption_or_body(),
                        }),
                        conditions: vec![],
                        ..Default::default()
                    },
                )])
                .collect(),
                invocations: vec![
                    std::array::IntoIter::new([(
                        s("name"),
                        ftd::Value::String {
                            text: s("hello"),
                            source: ftd::TextSource::Caption,
                        },
                    )])
                    .collect(),
                    std::array::IntoIter::new([(
                        s("name"),
                        ftd::Value::String {
                            text: s("world"),
                            source: ftd::TextSource::Header,
                        },
                    )])
                    .collect(),
                    std::array::IntoIter::new([(
                        s("name"),
                        ftd::Value::String {
                            text: s("yo yo"),
                            source: ftd::TextSource::Body,
                        },
                    )])
                    .collect(),
                ],
                line_number: 1,
                ..Default::default()
            }),
        );

        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("hello"),
                line: true,
                common: ftd::Common {
                    locals: std::array::IntoIter::new([(s("name@0"), s("hello"))]).collect(),
                    reference: Some(s("@name@0")),
                    ..Default::default()
                },
                ..Default::default()
            }));
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("world"),
                line: true,
                common: ftd::Common {
                    locals: std::array::IntoIter::new([(s("name@1"), s("world"))]).collect(),
                    reference: Some(s("@name@1")),
                    ..Default::default()
                },
                ..Default::default()
            }));
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("yo yo"),
                line: true,
                common: ftd::Common {
                    locals: std::array::IntoIter::new([(s("name@2"), s("yo yo"))]).collect(),
                    reference: Some(s("@name@2")),
                    ..Default::default()
                },
                ..Default::default()
            }));

        let (g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- ftd.text foo:
                caption or body name:
                text: $name

                -- foo: hello

                -- foo:
                name: world

                -- foo:

                yo yo
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        pretty_assertions::assert_eq!(g_bag, bag);
        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn row() {
        let mut main = super::default_column();
        let mut row = ftd::Row {
            common: ftd::Common {
                data_id: Some("the-row".to_string()),
                id: Some("the-row".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        row.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("hello"),
                line: true,
                ..Default::default()
            }));
        row.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("world"),
                line: true,
                ..Default::default()
            }));
        row.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("row child three"),
                line: true,
                ..Default::default()
            }));
        main.container.children.push(ftd::Element::Row(row));
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("back in main"),
                line: true,
                ..Default::default()
            }));

        p!(
            "
            -- ftd.row:
            id: the-row

            -- ftd.text:
            text: hello

            -- ftd.text:
            text: world

            -- container: ftd.main

            -- ftd.text:
            text: back in main

            -- container: the-row

            -- ftd.text:
            text: row child three
        ",
            (super::default_bag(), main),
        );
    }

    #[test]
    fn sub_function() {
        let mut main = super::default_column();
        let mut row: ftd::Row = Default::default();
        row.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("hello"),
                line: true,
                ..Default::default()
            }));
        row.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("world"),
                line: true,
                ..Default::default()
            }));
        main.container.children.push(ftd::Element::Row(row));
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("back in main"),
                line: true,
                ..Default::default()
            }));

        p!(
            "
            -- ftd.row:

            --- ftd.text:
            text: hello

            --- ftd.text:
            text: world

            -- ftd.text:
            text: back in main
        ",
            (super::default_bag(), main),
        );
    }

    #[test]
    fn sf1() {
        let mut bag = super::default_bag();

        bag.insert(
            "foo/bar#foo".to_string(),
            ftd::p2::Thing::Component(ftd::Component {
                full_name: s("foo/bar#foo"),
                root: "ftd.row".to_string(),
                instructions: vec![ftd::Instruction::ChildComponent{child: ftd::ChildComponent {
                    events: vec![],
                    condition: None,
                    root: s("ftd#text"),
                    properties: std::array::IntoIter::new([
                        (
                            s("text"),
                            ftd::component::Property {
                                default: Some(ftd::PropertyValue::Value {
                                    value: ftd::Value::String {
                                        text: s("hello"),
                                        source: ftd::TextSource::Header,
                                    },
                                }),
                                conditions: vec![],
                                ..Default::default()
                            },
                        ),
                        (
                            s("size"),
                            ftd::component::Property {
                                default: Some(ftd::PropertyValue::Value {
                                    value: ftd::Value::Integer { value: 14 },
                                }),
                                conditions: vec![],
                                ..Default::default()
                            },
                        ),
                        (
                            s("font"),
                            ftd::component::Property {
                                default: Some(ftd::PropertyValue::Value {
                                    value: ftd::Value::String {
                                        text: s("Roboto"),
                                        source: ftd::TextSource::Header,
                                    },
                                }),
                                conditions: vec![],
                                ..Default::default()
                            },
                        ),
                        (
                            s("font-url"),
                            ftd::component::Property {
                                default: Some(ftd::PropertyValue::Value {
                                    value: ftd::Value::String {
                                        text: s("https://fonts.googleapis.com/css2?family=Roboto:wght@100&display=swap"),
                                        source: ftd::TextSource::Header,
                                    },
                                }),
                                conditions: vec![],
                                ..Default::default()
                            },
                        ),
                        (
                            s("font-display"),
                            ftd::component::Property {
                                default: Some(ftd::PropertyValue::Value {
                                    value: ftd::Value::String {
                                        text: s("swap"),
                                        source: ftd::TextSource::Header,
                                    },
                                }),
                                conditions: vec![],
                                ..Default::default()
                            },
                        ),
                        (
                            s("border-width"),
                            ftd::component::Property {
                                default: Some(ftd::PropertyValue::Variable {
                                    name: s("x"),
                                    kind: ftd::p2::Kind::integer().into_optional(),
                                }),
                                conditions: vec![],
                                ..Default::default()
                            },
                        ),
                        (
                            s("overflow-x"),
                            ftd::component::Property {
                                default: Some(ftd::PropertyValue::Value {
                                    value: ftd::Value::String {
                                        text: s("auto"),
                                        source: ftd::TextSource::Header,
                                    },
                                }),
                                conditions: vec![],
                                ..Default::default()
                            },
                        ),
                        (
                            s("overflow-y"),
                            ftd::component::Property {
                                default: Some(ftd::PropertyValue::Value {
                                    value: ftd::Value::String {
                                        text: s("auto"),
                                        source: ftd::TextSource::Header,
                                    },
                                }),
                                conditions: vec![],
                                ..Default::default()
                            },
                        ),
                    ])
                    .collect(),
                    ..Default::default()
                }}],
                arguments: std::array::IntoIter::new([(s("x"), ftd::p2::Kind::integer())]).collect(),
                ..Default::default()
            }),
        );

        let mut main = super::default_column();
        let mut row: ftd::Row = Default::default();
        row.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("hello"),
                size: Some(14),
                external_font: Some(ftd::ExternalFont {
                    url: "https://fonts.googleapis.com/css2?family=Roboto:wght@100&display=swap"
                        .to_string(),
                    display: ftd::FontDisplay::Swap,
                    name: "Roboto".to_string(),
                }),
                font: vec![ftd::NamedFont::Named {
                    value: "Roboto".to_string(),
                }],

                line: true,
                common: ftd::Common {
                    border_width: 10,
                    overflow_x: Some(ftd::Overflow::Auto),
                    overflow_y: Some(ftd::Overflow::Auto),
                    ..Default::default()
                },
                ..Default::default()
            }));
        row.common = ftd::Common {
            locals: std::array::IntoIter::new([(s("x@0"), s("10"))]).collect(),
            ..Default::default()
        };

        main.container.children.push(ftd::Element::Row(row));
        p!(
            "
            -- ftd.row foo:
            integer x:

            --- ftd.text:
            text: hello
            size: 14
            border-width: $x
            font-url: https://fonts.googleapis.com/css2?family=Roboto:wght@100&display=swap
            font: Roboto
            font-display: swap
            overflow-x: auto
            overflow-y: auto

            -- foo:
            x: 10
        ",
            (bag, main),
        );
    }

    #[test]
    fn list_of_numbers() {
        let mut bag = super::default_bag();
        bag.insert(
            "foo/bar#numbers".to_string(),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: "foo/bar#numbers".to_string(),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::List {
                        data: vec![
                            ftd::PropertyValue::Value {
                                value: ftd::Value::Integer { value: 20 },
                            },
                            ftd::PropertyValue::Value {
                                value: ftd::Value::Integer { value: 30 },
                            },
                        ],
                        kind: ftd::p2::Kind::integer(),
                    },
                },
                conditions: vec![],
            }),
        );

        p!(
            "
            -- integer list numbers:

            -- numbers: 20
            -- numbers: 30
            ",
            (bag, super::default_column()),
        );
    }

    #[test]
    fn list_of_records() {
        let mut bag = super::default_bag();
        bag.insert(
            "foo/bar#point".to_string(),
            ftd::p2::Thing::Record(ftd::p2::Record {
                name: "foo/bar#point".to_string(),
                fields: std::array::IntoIter::new([
                    (s("x"), ftd::p2::Kind::integer()),
                    (s("y"), ftd::p2::Kind::integer()),
                ])
                .collect(),
                instances: Default::default(),
                order: vec![s("x"), s("y")],
            }),
        );

        bag.insert(
            "foo/bar#points".to_string(),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: "foo/bar#points".to_string(),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::List {
                        data: vec![
                            ftd::PropertyValue::Value {
                                value: ftd::Value::Record {
                                    name: s("foo/bar#point"),
                                    fields: std::array::IntoIter::new([
                                        (
                                            s("x"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::Value::Integer { value: 10 },
                                            },
                                        ),
                                        (
                                            s("y"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::Value::Integer { value: 20 },
                                            },
                                        ),
                                    ])
                                    .collect(),
                                },
                            },
                            ftd::PropertyValue::Value {
                                value: ftd::Value::Record {
                                    name: s("foo/bar#point"),
                                    fields: std::array::IntoIter::new([
                                        (
                                            s("x"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::Value::Integer { value: 0 },
                                            },
                                        ),
                                        (
                                            s("y"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::Value::Integer { value: 0 },
                                            },
                                        ),
                                    ])
                                    .collect(),
                                },
                            },
                        ],
                        kind: ftd::p2::Kind::Record {
                            name: s("foo/bar#point"),
                            default: None,
                        },
                    },
                },
                conditions: vec![],
            }),
        );

        p!(
            "
            -- record point:
            integer x:
            integer y:

            -- point list points:

            -- points:
            x: 10
            y: 20

            -- points:
            x: 0
            y: 0
            ",
            (bag, super::default_column()),
        );
    }

    #[test]
    #[ignore]
    fn list_with_reference() {
        let mut bag = super::default_bag();
        bag.insert(
            "foo/bar#numbers".to_string(),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: "foo/bar#numbers".to_string(),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::List {
                        data: vec![
                            ftd::PropertyValue::Value {
                                value: ftd::Value::Integer { value: 20 },
                            },
                            ftd::PropertyValue::Value {
                                value: ftd::Value::Integer { value: 30 },
                            },
                            // TODO: third element
                        ],
                        kind: ftd::p2::Kind::integer(),
                    },
                },
                conditions: vec![],
            }),
        );
        bag.insert(
            "foo/bar#x".to_string(),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: "x".to_string(),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::Integer { value: 20 },
                },
                conditions: vec![],
            }),
        );

        p!(
            "
            -- integer list numbers:

            -- numbers: 20
            -- numbers: 30

            -- integer x: 20

            -- numbers: $x
            ",
            (bag, super::default_column()),
        );
    }

    fn white_two_image_bag(
        about_optional: bool,
    ) -> std::collections::BTreeMap<String, ftd::p2::Thing> {
        let mut bag = super::default_bag();
        bag.insert(
            s("foo/bar#white-two-image"),
            ftd::p2::Thing::Component(ftd::Component {
                invocations: Default::default(),
                full_name: "foo/bar#white-two-image".to_string(),
                root: s("ftd.column"),
                arguments: std::array::IntoIter::new([
                    (s("about"), {
                        let s = ftd::p2::Kind::body();
                        if about_optional {
                            s.into_optional()
                        } else {
                            s
                        }
                    }),
                    (s("src"), {
                        let s = ftd::p2::Kind::string();
                        if about_optional {
                            s.into_optional()
                        } else {
                            s
                        }
                    }),
                    (s("title"), ftd::p2::Kind::caption()),
                ])
                .collect(),
                properties: std::array::IntoIter::new([(
                    s("padding"),
                    ftd::component::Property {
                        default: Some(ftd::PropertyValue::Value {
                            value: ftd::Value::Integer { value: 30 },
                        }),
                        conditions: vec![],
                        ..Default::default()
                    },
                )])
                .collect(),
                kernel: false,
                instructions: vec![
                    ftd::Instruction::ChildComponent {
                        child: ftd::ChildComponent {
                            events: vec![],
                            condition: None,
                            root: s("ftd#text"),
                            properties: std::array::IntoIter::new([
                                (
                                    s("text"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Variable {
                                            name: s("title"),
                                            kind: ftd::p2::Kind::caption_or_body(),
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                                (
                                    s("align"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::Value::String {
                                                source: ftd::TextSource::Header,
                                                text: s("center"),
                                            },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                            ])
                            .collect(),
                            ..Default::default()
                        },
                    },
                    ftd::Instruction::ChildComponent {
                        child: ftd::ChildComponent {
                            events: vec![],
                            condition: if about_optional {
                                Some(ftd::p2::Boolean::IsNotNull {
                                    value: ftd::PropertyValue::Variable {
                                        name: s("about"),
                                        kind: ftd::p2::Kind::body().into_optional(),
                                    },
                                })
                            } else {
                                None
                            },
                            root: s("ftd#text"),
                            properties: std::array::IntoIter::new([(
                                s("text"),
                                ftd::component::Property {
                                    default: Some(ftd::PropertyValue::Variable {
                                        name: s("about"),
                                        kind: ftd::p2::Kind::caption_or_body(),
                                    }),
                                    conditions: vec![],
                                    ..Default::default()
                                },
                            )])
                            .collect(),
                            ..Default::default()
                        },
                    },
                    ftd::Instruction::ChildComponent {
                        child: ftd::ChildComponent {
                            events: vec![],
                            condition: if about_optional {
                                Some(ftd::p2::Boolean::IsNotNull {
                                    value: ftd::PropertyValue::Variable {
                                        name: s("src"),
                                        kind: ftd::p2::Kind::string().into_optional(),
                                    },
                                })
                            } else {
                                None
                            },
                            root: s("ftd#image"),
                            properties: std::array::IntoIter::new([(
                                s("src"),
                                ftd::component::Property {
                                    default: Some(ftd::PropertyValue::Variable {
                                        name: s("src"),
                                        kind: ftd::p2::Kind::string(),
                                    }),
                                    conditions: vec![],
                                    ..Default::default()
                                },
                            )])
                            .collect(),
                            ..Default::default()
                        },
                    },
                ],
                ..Default::default()
            }),
        );
        bag
    }

    #[test]
    fn components() {
        let title = ftd::Markups {
            text: ftd::markup_line("What kind of documentation?"),
            line: true,
            common: ftd::Common {
                position: Some(ftd::Position::Center),
                reference: Some(s("@title@0")),
                ..Default::default()
            },
            ..Default::default()
        };
        let about = ftd::Markups {
            text: ftd::markup_line(
                indoc::indoc!(
                    "
                    UI screens, behaviour and journeys, database tables, APIs, how to
                    contribute to, deploy, or monitor microservice, everything that
                    makes web or mobile product teams productive.
                    "
                )
                .trim(),
            ),
            common: ftd::Common {
                reference: Some(s("@about@0")),
                ..Default::default()
            },
            line: true,
            ..Default::default()
        };

        let image = ftd::Image {
            src: s("/static/home/document-type-min.png"),
            common: ftd::Common {
                reference: Some(s("@src@0")),
                ..Default::default()
            },
            ..Default::default()
        };

        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {spacing: None,
                common: ftd::Common {
                    padding: Some(30),
                    locals: std::array::IntoIter::new([
                        (s("about@0"), s("UI screens, behaviour and journeys, database tables, APIs, how to\ncontribute to, deploy, or monitor microservice, everything that\nmakes web or mobile product teams productive.")),
                        (
                            s("src@0"),
                            s("/static/home/document-type-min.png"),
                        ),
                        (s("title@0"), s("What kind of documentation?")),
                    ])
                    .collect(),
                    ..Default::default()
                },
                container: ftd::Container {
                    children: vec![
                        ftd::Element::Markup(title),
                        ftd::Element::Markup(about),
                        ftd::Element::Image(image),
                    ],
                    ..Default::default()
                },
            }));

        p!(
            "
            -- ftd.column white-two-image:
            caption title:
            body about:
            string src:
            padding: 30

            --- ftd.text:
            text: $title
            align: center

            --- ftd.text:
            text: $about

            --- ftd.image:
            src: $src

            -- white-two-image: What kind of documentation?
            src: /static/home/document-type-min.png

            UI screens, behaviour and journeys, database tables, APIs, how to
            contribute to, deploy, or monitor microservice, everything that
            makes web or mobile product teams productive.
            ",
            (white_two_image_bag(false), main),
        );
    }

    #[test]
    fn conditional_body() {
        let title = ftd::Markups {
            text: ftd::markup_line("What kind of documentation?"),
            common: ftd::Common {
                position: Some(ftd::Position::Center),
                reference: Some(s("@title@0")),
                ..Default::default()
            },
            line: true,
            ..Default::default()
        };
        let second_title = ftd::Markups {
            text: ftd::markup_line("second call"),
            common: ftd::Common {
                position: Some(ftd::Position::Center),
                reference: Some(s("@title@1")),
                ..Default::default()
            },
            line: true,
            ..Default::default()
        };
        let about = ftd::Markups {
            text: ftd::markup_line(
                indoc::indoc!(
                    "
                    UI screens, behaviour and journeys, database tables, APIs, how to
                    contribute to, deploy, or monitor microservice, everything that
                    makes web or mobile product teams productive.
                    "
                )
                .trim(),
            ),
            common: ftd::Common {
                reference: Some(s("@about@0")),
                condition: Some(ftd::Condition {
                    variable: s("@about@0"),
                    value: s("$IsNotNull$"),
                }),
                ..Default::default()
            },
            line: true,
            ..Default::default()
        };
        let second_about = ftd::Markups {
            text: ftd::markup_line(""),
            common: ftd::Common {
                reference: Some(s("@about@1")),
                condition: Some(ftd::Condition {
                    variable: s("@about@1"),
                    value: s("$IsNotNull$"),
                }),
                is_not_visible: true,
                ..Default::default()
            },
            line: true,
            ..Default::default()
        };
        let image = ftd::Image {
            src: s("/static/home/document-type-min.png"),
            common: ftd::Common {
                reference: Some(s("@src@0")),
                condition: Some(ftd::Condition {
                    variable: s("@src@0"),
                    value: s("$IsNotNull$"),
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        let second_image = ftd::Image {
            src: s("second-image.png"),
            common: ftd::Common {
                reference: Some(s("@src@1")),
                condition: Some(ftd::Condition {
                    variable: s("@src@1"),
                    value: s("$IsNotNull$"),
                }),
                ..Default::default()
            },
            ..Default::default()
        };

        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {spacing: None,
                common: ftd::Common {
                    padding: Some(30),
                    locals: std::array::IntoIter::new([
                        (s("about@0"), s("UI screens, behaviour and journeys, database tables, APIs, how to\ncontribute to, deploy, or monitor microservice, everything that\nmakes web or mobile product teams productive.")),
                        (
                            s("src@0"),
                            s("/static/home/document-type-min.png"),
                        ),
                        (
                            s("title@0"),
                            s("What kind of documentation?"),
                        ),
                    ])
                    .collect(),
                    ..Default::default()
                },
                container: ftd::Container {
                    children: vec![
                        ftd::Element::Markup(title),
                        ftd::Element::Markup(about),
                        ftd::Element::Image(image),
                    ],
                    ..Default::default()
                },
            }));
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                common: ftd::Common {
                    padding: Some(30),
                    locals: std::array::IntoIter::new([
                        (s("about@1"), s("")),
                        (s("src@1"), s("second-image.png")),
                        (s("title@1"), s("second call")),
                    ])
                    .collect(),
                    ..Default::default()
                },
                container: ftd::Container {
                    children: vec![
                        ftd::Element::Markup(second_title),
                        ftd::Element::Markup(second_about),
                        ftd::Element::Image(second_image),
                    ],
                    ..Default::default()
                },
            }));

        p!(
            "
            -- ftd.column white-two-image:
            caption title:
            optional body about:
            optional string src:
            padding: 30

            --- ftd.text:
            text: $title
            align: center

            --- ftd.text:
            if: $about is not null
            text: $about

            --- ftd.image:
            if: $src is not null
            src: $src

            -- white-two-image: What kind of documentation?
            src: /static/home/document-type-min.png

            UI screens, behaviour and journeys, database tables, APIs, how to
            contribute to, deploy, or monitor microservice, everything that
            makes web or mobile product teams productive.

            -- white-two-image: second call
            src: second-image.png
            ",
            (white_two_image_bag(true), main),
        );
    }

    #[test]
    fn conditional_header() {
        let title = ftd::Markups {
            text: ftd::markup_line("What kind of documentation?"),
            common: ftd::Common {
                position: Some(ftd::Position::Center),
                reference: Some(s("@title@0")),
                ..Default::default()
            },
            line: true,
            ..Default::default()
        };
        let second_title = ftd::Markups {
            text: ftd::markup_line("second call"),
            common: ftd::Common {
                position: Some(ftd::Position::Center),
                reference: Some(s("@title@1")),
                ..Default::default()
            },
            line: true,
            ..Default::default()
        };
        let third_title = ftd::Markups {
            text: ftd::markup_line("third call"),
            common: ftd::Common {
                position: Some(ftd::Position::Center),
                reference: Some(s("@title@2")),
                ..Default::default()
            },
            line: true,
            ..Default::default()
        };
        let about = ftd::Markups {
            text: ftd::markup_line(
                indoc::indoc!(
                    "
                    UI screens, behaviour and journeys, database tables, APIs, how to
                    contribute to, deploy, or monitor microservice, everything that
                    makes web or mobile product teams productive.
                    "
                )
                .trim(),
            ),
            common: ftd::Common {
                condition: Some(ftd::Condition {
                    variable: s("@about@0"),
                    value: s("$IsNotNull$"),
                }),
                reference: Some(s("@about@0")),
                ..Default::default()
            },
            line: true,
            ..Default::default()
        };
        let second_about = ftd::Markups {
            text: ftd::markup_line(""),
            common: ftd::Common {
                condition: Some(ftd::Condition {
                    variable: s("@about@1"),
                    value: s("$IsNotNull$"),
                }),
                reference: Some(s("@about@1")),
                is_not_visible: true,
                ..Default::default()
            },
            line: true,
            ..Default::default()
        };
        let third_about = ftd::Markups {
            text: ftd::markup_line(""),
            common: ftd::Common {
                condition: Some(ftd::Condition {
                    variable: s("@about@2"),
                    value: s("$IsNotNull$"),
                }),
                reference: Some(s("@about@2")),
                is_not_visible: true,
                ..Default::default()
            },
            line: true,
            ..Default::default()
        };
        let image = ftd::Image {
            src: s("/static/home/document-type-min.png"),
            common: ftd::Common {
                reference: Some(s("@src@0")),
                condition: Some(ftd::Condition {
                    variable: s("@src@0"),
                    value: s("$IsNotNull$"),
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        let second_image = ftd::Image {
            src: s("second-image.png"),
            common: ftd::Common {
                reference: Some(s("@src@1")),
                condition: Some(ftd::Condition {
                    variable: s("@src@1"),
                    value: s("$IsNotNull$"),
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        let third_image = ftd::Image {
            src: s(""),
            common: ftd::Common {
                reference: Some(s("@src@2")),
                condition: Some(ftd::Condition {
                    variable: s("@src@2"),
                    value: s("$IsNotNull$"),
                }),
                is_not_visible: true,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {spacing: None,
                common: ftd::Common {
                    padding: Some(30),
                    locals: std::array::IntoIter::new([
                        (s("about@0"), s("UI screens, behaviour and journeys, database tables, APIs, how to\ncontribute to, deploy, or monitor microservice, everything that\nmakes web or mobile product teams productive.")),
                        (
                            s("src@0"),
                            s("/static/home/document-type-min.png"),
                        ),
                        (
                            s("title@0"),
                            s("What kind of documentation?"),
                        ),
                    ])
                    .collect(),
                    ..Default::default()
                },
                container: ftd::Container {
                    children: vec![
                        ftd::Element::Markup(title),
                        ftd::Element::Markup(about),
                        ftd::Element::Image(image),
                    ],
                    ..Default::default()
                },
            }));
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                common: ftd::Common {
                    padding: Some(30),
                    locals: std::array::IntoIter::new([
                        (s("about@1"), s("")),
                        (s("src@1"), s("second-image.png")),
                        (s("title@1"), s("second call")),
                    ])
                    .collect(),
                    ..Default::default()
                },
                container: ftd::Container {
                    children: vec![
                        ftd::Element::Markup(second_title),
                        ftd::Element::Markup(second_about),
                        ftd::Element::Image(second_image),
                    ],
                    ..Default::default()
                },
            }));
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                common: ftd::Common {
                    padding: Some(30),
                    locals: std::array::IntoIter::new([
                        (s("about@2"), s("")),
                        (s("src@2"), s("")),
                        (s("title@2"), s("third call")),
                    ])
                    .collect(),
                    ..Default::default()
                },
                container: ftd::Container {
                    children: vec![
                        ftd::Element::Markup(third_title),
                        ftd::Element::Markup(third_about),
                        ftd::Element::Image(third_image),
                    ],
                    ..Default::default()
                },
            }));

        p!(
            "
            -- ftd.column white-two-image:
            caption title:
            optional body about:
            optional string src:
            padding: 30

            --- ftd.text:
            text: $title
            align: center

            --- ftd.text:
            if: $about is not null
            text: $about

            --- ftd.image:
            if: $src is not null
            src: $src

            -- white-two-image: What kind of documentation?
            src: /static/home/document-type-min.png

            UI screens, behaviour and journeys, database tables, APIs, how to
            contribute to, deploy, or monitor microservice, everything that
            makes web or mobile product teams productive.

            -- white-two-image: second call
            src: second-image.png

            -- white-two-image: third call
            ",
            (white_two_image_bag(true), main),
        );
    }

    #[test]
    fn markdown() {
        let mut bag = super::default_bag();
        bag.insert(
            s("fifthtry/ft#markdown"),
            ftd::p2::Thing::Component(ftd::Component {
                invocations: Default::default(),
                full_name: "fifthtry/ft#markdown".to_string(),
                root: s("ftd.text"),
                arguments: std::array::IntoIter::new([(s("body"), ftd::p2::Kind::body())])
                    .collect(),
                properties: std::array::IntoIter::new([(
                    s("text"),
                    ftd::component::Property {
                        default: Some(ftd::PropertyValue::Variable {
                            name: s("body"),
                            kind: ftd::p2::Kind::string().string_any(),
                        }),
                        conditions: vec![],
                        ..Default::default()
                    },
                )])
                .collect(),
                ..Default::default()
            }),
        );
        bag.insert(
            s("fifthtry/ft#dark-mode"),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: s("dark-mode"),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::Boolean { value: true },
                },
                conditions: vec![],
            }),
        );
        bag.insert(
            s("fifthtry/ft#toc"),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: s("toc"),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::String {
                        text: "not set".to_string(),
                        source: ftd::TextSource::Caption,
                    },
                },
                conditions: vec![],
            }),
        );
        bag.insert(
            s("foo/bar#h0"),
            ftd::p2::Thing::Component(ftd::Component {
                invocations: Default::default(),
                full_name: "foo/bar#h0".to_string(),
                root: s("ftd.column"),
                arguments: std::array::IntoIter::new([
                    (s("body"), ftd::p2::Kind::body().into_optional()),
                    (s("title"), ftd::p2::Kind::caption()),
                ])
                .collect(),
                instructions: vec![
                    ftd::Instruction::ChildComponent {
                        child: ftd::ChildComponent {
                            events: vec![],
                            condition: None,
                            root: s("ftd#text"),
                            properties: std::array::IntoIter::new([(
                                s("text"),
                                ftd::component::Property {
                                    default: Some(ftd::PropertyValue::Variable {
                                        name: s("title"),
                                        kind: ftd::p2::Kind::caption_or_body(),
                                    }),
                                    conditions: vec![],
                                    ..Default::default()
                                },
                            )])
                            .collect(),
                            ..Default::default()
                        },
                    },
                    ftd::Instruction::ChildComponent {
                        child: ftd::ChildComponent {
                            events: vec![],
                            condition: Some(ftd::p2::Boolean::IsNotNull {
                                value: ftd::PropertyValue::Variable {
                                    name: s("body"),
                                    kind: ftd::p2::Kind::body().into_optional(),
                                },
                            }),
                            root: s("fifthtry/ft#markdown"),
                            properties: std::array::IntoIter::new([(
                                s("body"),
                                ftd::component::Property {
                                    default: Some(ftd::PropertyValue::Variable {
                                        name: s("body"),
                                        kind: ftd::p2::Kind::body(),
                                    }),
                                    conditions: vec![],
                                    ..Default::default()
                                },
                            )])
                            .collect(),
                            ..Default::default()
                        },
                    },
                ],
                ..Default::default()
            }),
        );

        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("hello"),
                            line: true,
                            common: ftd::Common {
                                reference: Some(s("@title@0")),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("what about the body?"),
                            line: true,
                            common: ftd::Common {
                                locals: std::array::IntoIter::new([(
                                    s("body@0,1"),
                                    s("what about the body?"),
                                )])
                                .collect(),
                                condition: Some(ftd::Condition {
                                    variable: s("@body@0"),
                                    value: s("$IsNotNull$"),
                                }),
                                reference: Some(s("@body@0,1")),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                    ],
                    ..Default::default()
                },
                common: ftd::Common {
                    locals: std::array::IntoIter::new([
                        (s("body@0"), s("what about the body?")),
                        (s("title@0"), s("hello")),
                    ])
                    .collect(),
                    ..Default::default()
                },
            }));
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("heading without body"),
                            line: true,
                            common: ftd::Common {
                                reference: Some(s("@title@1")),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line(""),
                            line: true,
                            common: ftd::Common {
                                locals: std::array::IntoIter::new([(s("body@1,1"), s(""))])
                                    .collect(),
                                condition: Some(ftd::Condition {
                                    variable: s("@body@1"),
                                    value: s("$IsNotNull$"),
                                }),
                                reference: Some(s("@body@1,1")),
                                is_not_visible: true,
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                    ],
                    ..Default::default()
                },
                common: ftd::Common {
                    locals: std::array::IntoIter::new([
                        (s("body@1"), s("")),
                        (s("title@1"), s("heading without body")),
                    ])
                    .collect(),
                    ..Default::default()
                },
            }));

        p!(
            "
            -- import: fifthtry/ft

            -- ftd.column h0:
            caption title:
            optional body body:

            --- ftd.text:
            text: $title

            --- ft.markdown:
            if: $body is not null
            body: $body

            -- h0: hello

            what about the body?

            -- h0: heading without body
            ",
            (bag, main),
        );
    }

    #[test]
    fn width() {
        let mut bag = super::default_bag();

        bag.insert(
            s("foo/bar#image"),
            ftd::p2::Thing::Component(ftd::Component {
                invocations: Default::default(),
                full_name: "foo/bar#image".to_string(),
                root: s("ftd.column"),
                arguments: std::array::IntoIter::new([
                    (s("width"), ftd::p2::Kind::string().into_optional()),
                    (s("src"), ftd::p2::Kind::string()),
                ])
                .collect(),
                instructions: vec![ftd::Instruction::ChildComponent {
                    child: ftd::ChildComponent {
                        events: vec![],
                        condition: None,
                        root: s("ftd#image"),
                        properties: std::array::IntoIter::new([
                            (
                                s("src"),
                                ftd::component::Property {
                                    default: Some(ftd::PropertyValue::Variable {
                                        name: s("src"),
                                        kind: ftd::p2::Kind::string(),
                                    }),
                                    conditions: vec![],
                                    ..Default::default()
                                },
                            ),
                            (
                                s("width"),
                                ftd::component::Property {
                                    default: Some(ftd::PropertyValue::Variable {
                                        name: s("width"),
                                        kind: ftd::p2::Kind::string().into_optional(),
                                    }),
                                    conditions: vec![],
                                    ..Default::default()
                                },
                            ),
                        ])
                        .collect(),
                        ..Default::default()
                    },
                }],
                ..Default::default()
            }),
        );

        let mut main = super::default_column();

        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![ftd::Element::Image(ftd::Image {
                        src: s("foo.png"),
                        common: ftd::Common {
                            reference: Some(s("@src@0")),
                            ..Default::default()
                        },
                        ..Default::default()
                    })],
                    ..Default::default()
                },
                common: ftd::Common {
                    locals: std::array::IntoIter::new([
                        (s("src@0"), s("foo.png")),
                        (s("width@0"), s("")),
                    ])
                    .collect(),
                    ..Default::default()
                },
            }));
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![ftd::Element::Image(ftd::Image {
                        src: s("bar.png"),
                        common: ftd::Common {
                            reference: Some(s("@src@1")),
                            width: Some(ftd::Length::Px { value: 300 }),
                            ..Default::default()
                        },
                        ..Default::default()
                    })],
                    ..Default::default()
                },
                common: ftd::Common {
                    locals: std::array::IntoIter::new([
                        (s("src@1"), s("bar.png")),
                        (s("width@1"), s("300")),
                    ])
                    .collect(),
                    ..Default::default()
                },
            }));

        p!(
            "
            -- ftd.column image:
            string src:
            optional string width:

            --- ftd.image:
            src: $src
            width: $width

            -- image:
            src: foo.png

            -- image:
            src: bar.png
            width: 300
            ",
            (bag, main),
        );
    }

    #[test]
    fn decimal() {
        let mut bag = super::default_bag();

        bag.insert(
            "foo/bar#foo".to_string(),
            ftd::p2::Thing::Component(ftd::Component {
                full_name: s("foo/bar#foo"),
                root: "ftd.row".to_string(),
                instructions: vec![
                    ftd::Instruction::ChildComponent {
                        child: ftd::ChildComponent {
                            events: vec![],
                            condition: None,
                            root: s("ftd#decimal"),
                            properties: std::array::IntoIter::new([
                                (
                                    s("value"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::Value::Decimal { value: 0.06 },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                                (
                                    s("format"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::Value::String {
                                                text: s(".1f"),
                                                source: ftd::TextSource::Header,
                                            },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                            ])
                            .collect(),
                            ..Default::default()
                        },
                    },
                    ftd::Instruction::ChildComponent {
                        child: ftd::ChildComponent {
                            events: vec![],
                            condition: None,
                            root: s("ftd#decimal"),
                            properties: std::array::IntoIter::new([(
                                s("value"),
                                ftd::component::Property {
                                    default: Some(ftd::PropertyValue::Value {
                                        value: ftd::Value::Decimal { value: 0.01 },
                                    }),
                                    conditions: vec![],
                                    ..Default::default()
                                },
                            )])
                            .collect(),
                            ..Default::default()
                        },
                    },
                ],
                arguments: std::array::IntoIter::new([(s("x"), ftd::p2::Kind::integer())])
                    .collect(),
                ..Default::default()
            }),
        );

        let mut main = super::default_column();
        let mut row: ftd::Row = Default::default();
        row.container
            .children
            .push(ftd::Element::Decimal(ftd::Text {
                text: ftd::markdown_line("0.1"),
                line: false,
                ..Default::default()
            }));
        row.container
            .children
            .push(ftd::Element::Decimal(ftd::Text {
                text: ftd::markdown_line("0.01"),
                line: false,
                ..Default::default()
            }));
        row.common = ftd::Common {
            locals: std::array::IntoIter::new([(s("x@0"), s("10"))]).collect(),
            ..Default::default()
        };
        main.container.children.push(ftd::Element::Row(row));

        p!(
            "
            -- ftd.row foo:
            integer x:

            --- ftd.decimal:
            value: 0.06
            format: .1f

            --- ftd.decimal:
            value: 0.01

            -- foo:
            x: 10
        ",
            (bag, main),
        );
    }

    #[test]
    fn integer() {
        let mut bag = super::default_bag();

        bag.insert(
            "foo/bar#foo".to_string(),
            ftd::p2::Thing::Component(ftd::Component {
                full_name: s("foo/bar#foo"),
                root: "ftd.row".to_string(),
                instructions: vec![
                    ftd::Instruction::ChildComponent {
                        child: ftd::ChildComponent {
                            events: vec![],
                            condition: None,
                            root: s("ftd#integer"),
                            properties: std::array::IntoIter::new([
                                (
                                    s("value"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::Value::Integer { value: 3 },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                                (
                                    s("format"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::Value::String {
                                                text: s("b"),
                                                source: ftd::TextSource::Header,
                                            },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                            ])
                            .collect(),
                            ..Default::default()
                        },
                    },
                    ftd::Instruction::ChildComponent {
                        child: ftd::ChildComponent {
                            events: vec![],
                            condition: None,
                            root: s("ftd#integer"),
                            properties: std::array::IntoIter::new([(
                                s("value"),
                                ftd::component::Property {
                                    default: Some(ftd::PropertyValue::Value {
                                        value: ftd::Value::Integer { value: 14 },
                                    }),
                                    conditions: vec![],
                                    ..Default::default()
                                },
                            )])
                            .collect(),
                            ..Default::default()
                        },
                    },
                ],
                arguments: std::array::IntoIter::new([(s("x"), ftd::p2::Kind::integer())])
                    .collect(),
                ..Default::default()
            }),
        );

        let mut main = super::default_column();
        let mut row: ftd::Row = Default::default();
        row.container
            .children
            .push(ftd::Element::Integer(ftd::Text {
                text: ftd::markdown_line("11"),
                line: false,
                ..Default::default()
            }));
        row.container
            .children
            .push(ftd::Element::Integer(ftd::Text {
                text: ftd::markdown_line("14"),
                line: false,
                ..Default::default()
            }));

        row.common = ftd::Common {
            locals: std::array::IntoIter::new([(s("x@0"), s("10"))]).collect(),
            ..Default::default()
        };

        main.container.children.push(ftd::Element::Row(row));

        p!(
            "
            -- ftd.row foo:
            integer x:

            --- ftd.integer:
            value: 3
            format: b

            --- ftd.integer:
            value: 14

            -- foo:
            x: 10
        ",
            (bag, main),
        );
    }

    #[test]
    fn boolean() {
        let mut bag = super::default_bag();

        bag.insert(
            "foo/bar#foo".to_string(),
            ftd::p2::Thing::Component(ftd::Component {
                full_name: s("foo/bar#foo"),
                root: "ftd.row".to_string(),
                instructions: vec![
                    ftd::Instruction::ChildComponent {
                        child: ftd::ChildComponent {
                            events: vec![],
                            condition: None,
                            root: s("ftd#boolean"),
                            properties: std::array::IntoIter::new([
                                (
                                    s("value"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::Value::Boolean { value: true },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                                (
                                    s("true"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::Value::String {
                                                text: s("show this when value is true"),
                                                source: ftd::TextSource::Header,
                                            },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                                (
                                    s("false"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::Value::String {
                                                text: s("show this when value is false"),
                                                source: ftd::TextSource::Header,
                                            },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                            ])
                            .collect(),
                            ..Default::default()
                        },
                    },
                    ftd::Instruction::ChildComponent {
                        child: ftd::ChildComponent {
                            events: vec![],
                            condition: None,
                            root: s("ftd#boolean"),
                            properties: std::array::IntoIter::new([
                                (
                                    s("value"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::Value::Boolean { value: false },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                                (
                                    s("true"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::Value::String {
                                                text: s("show this when value is true"),
                                                source: ftd::TextSource::Header,
                                            },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                                (
                                    s("false"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Value {
                                            value: ftd::Value::String {
                                                text: s("show this when value is false"),
                                                source: ftd::TextSource::Header,
                                            },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                            ])
                            .collect(),
                            ..Default::default()
                        },
                    },
                ],
                arguments: std::array::IntoIter::new([(s("x"), ftd::p2::Kind::integer())])
                    .collect(),
                ..Default::default()
            }),
        );

        let mut main = super::default_column();
        let mut row: ftd::Row = Default::default();
        row.container
            .children
            .push(ftd::Element::Boolean(ftd::Text {
                text: ftd::markdown_line("show this when value is true"),
                line: false,
                ..Default::default()
            }));
        row.container
            .children
            .push(ftd::Element::Boolean(ftd::Text {
                text: ftd::markdown_line("show this when value is false"),
                line: false,
                ..Default::default()
            }));
        row.common = ftd::Common {
            locals: std::array::IntoIter::new([(s("x@0"), s("10"))]).collect(),
            ..Default::default()
        };
        main.container.children.push(ftd::Element::Row(row));

        p!(
            "
            -- ftd.row foo:
            integer x:

            --- ftd.boolean:
            value: true
            true:  show this when value is true
            false: show this when value is false

            --- ftd.boolean:
            value: false
            true:  show this when value is true
            false: show this when value is false

            -- foo:
            x: 10
        ",
            (bag, main),
        );
    }

    #[test]
    fn boolean_expression() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("present is true"),
                line: true,
                common: ftd::Common {
                    condition: Some(ftd::Condition {
                        variable: "foo/bar#present".to_string(),
                        value: "true".to_string(),
                    }),
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("present is false"),
                line: true,
                common: ftd::Common {
                    condition: Some(ftd::Condition {
                        variable: "foo/bar#present".to_string(),
                        value: "false".to_string(),
                    }),
                    is_not_visible: true,
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("dark-mode is true"),
                line: true,
                common: ftd::Common {
                    condition: Some(ftd::Condition {
                        variable: "fifthtry/ft#dark-mode".to_string(),
                        value: "true".to_string(),
                    }),
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("dark-mode is false"),
                line: true,
                common: ftd::Common {
                    condition: Some(ftd::Condition {
                        variable: "fifthtry/ft#dark-mode".to_string(),
                        value: "false".to_string(),
                    }),
                    is_not_visible: true,
                    ..Default::default()
                },
                ..Default::default()
            }));

        let mut column: ftd::Column = Default::default();
        column
            .container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("inner present false"),
                line: true,
                common: ftd::Common {
                    condition: Some(ftd::Condition {
                        variable: "foo/bar#present".to_string(),
                        value: "false".to_string(),
                    }),
                    is_not_visible: true,
                    ..Default::default()
                },
                ..Default::default()
            }));

        column
            .container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("inner present true"),
                line: true,
                common: ftd::Common {
                    condition: Some(ftd::Condition {
                        variable: "foo/bar#present".to_string(),
                        value: "true".to_string(),
                    }),
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container.children.push(ftd::Element::Column(column));

        let mut column: ftd::Column = Default::default();
        column
            .container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("argument present false"),
                line: true,
                common: ftd::Common {
                    condition: Some(ftd::Condition {
                        variable: s("@present@5"),
                        value: s("false"),
                    }),
                    ..Default::default()
                },
                ..Default::default()
            }));
        column
            .container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("argument present true"),
                line: true,
                common: ftd::Common {
                    condition: Some(ftd::Condition {
                        variable: s("@present@5"),
                        value: s("true"),
                    }),
                    is_not_visible: true,
                    ..Default::default()
                },
                ..Default::default()
            }));

        column.common = ftd::Common {
            locals: std::array::IntoIter::new([(s("present@5"), s("false"))]).collect(),
            ..Default::default()
        };

        main.container.children.push(ftd::Element::Column(column));

        let mut column: ftd::Column = Default::default();
        column
            .container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("argument present false"),
                line: true,
                common: ftd::Common {
                    condition: Some(ftd::Condition {
                        variable: s("@present@6"),
                        value: s("false"),
                    }),
                    is_not_visible: true,
                    ..Default::default()
                },
                ..Default::default()
            }));
        column
            .container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("argument present true"),
                line: true,
                common: ftd::Common {
                    condition: Some(ftd::Condition {
                        variable: s("@present@6"),
                        value: s("true"),
                    }),
                    ..Default::default()
                },
                ..Default::default()
            }));

        column.common = ftd::Common {
            locals: std::array::IntoIter::new([(s("present@6"), s("true"))]).collect(),
            ..Default::default()
        };

        main.container.children.push(ftd::Element::Column(column));

        let mut column: ftd::Column = Default::default();
        column
            .container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("foo2 dark-mode is true"),
                line: true,
                common: ftd::Common {
                    condition: Some(ftd::Condition {
                        variable: "fifthtry/ft#dark-mode".to_string(),
                        value: "true".to_string(),
                    }),
                    ..Default::default()
                },
                ..Default::default()
            }));

        column
            .container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("foo2 dark-mode is false"),
                line: true,
                common: ftd::Common {
                    condition: Some(ftd::Condition {
                        variable: "fifthtry/ft#dark-mode".to_string(),
                        value: "false".to_string(),
                    }),
                    is_not_visible: true,
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container.children.push(ftd::Element::Column(column));

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("hello literal truth"),
                line: true,
                ..Default::default()
            }));

        main.container.children.push(ftd::Element::Null);

        p!(
            "
            -- import: fifthtry/ft
            -- boolean present: true

            -- ftd.text: present is true
            if: $present

            -- ftd.text: present is false
            if: not $present

            -- ftd.text: dark-mode is true
            if: $ft.dark-mode

            -- ftd.text: dark-mode is false
            if: not $ft.dark-mode

            -- ftd.column foo:

            --- ftd.text: inner present false
            if: not $present

            --- ftd.text: inner present true
            if: $present

            -- foo:

            -- ftd.column bar:
            boolean present:

            --- ftd.text: argument present false
            if: not $present

            --- ftd.text: argument present true
            if: $present

            -- bar:
            present: false

            -- bar:
            present: $ft.dark-mode

            -- ftd.column foo2:

            --- ftd.text: foo2 dark-mode is true
            if: $ft.dark-mode

            --- ftd.text: foo2 dark-mode is false
            if: not $ft.dark-mode

            -- foo2:

            -- ftd.text: hello literal truth
            if: true

            -- ftd.text: never see light of the day
            if: false
        ",
            (Default::default(), main),
        );
    }

    #[test]
    fn inner_container() {
        let mut bag = super::default_bag();

        bag.insert(
            "foo/bar#foo".to_string(),
            ftd::p2::Thing::Component(ftd::Component {
                root: "ftd.column".to_string(),
                full_name: "foo/bar#foo".to_string(),
                instructions: vec![
                    ftd::component::Instruction::ChildComponent {
                        child: ftd::component::ChildComponent {
                            is_recursive: false,
                            events: vec![],
                            root: "ftd#row".to_string(),
                            condition: None,
                            properties: std::array::IntoIter::new([(
                                s("id"),
                                ftd::component::Property {
                                    default: Some(ftd::PropertyValue::Value {
                                        value: ftd::variable::Value::String {
                                            text: "r1".to_string(),
                                            source: ftd::TextSource::Header,
                                        },
                                    }),
                                    conditions: vec![],
                                    ..Default::default()
                                },
                            )])
                            .collect(),
                            ..Default::default()
                        },
                    },
                    ftd::component::Instruction::ChildComponent {
                        child: ftd::component::ChildComponent {
                            is_recursive: false,
                            events: vec![],
                            root: "ftd#row".to_string(),
                            condition: None,
                            properties: std::array::IntoIter::new([(
                                s("id"),
                                ftd::component::Property {
                                    default: Some(ftd::PropertyValue::Value {
                                        value: ftd::variable::Value::String {
                                            text: "r2".to_string(),
                                            source: ftd::TextSource::Header,
                                        },
                                    }),
                                    conditions: vec![],
                                    ..Default::default()
                                },
                            )])
                            .collect(),
                            ..Default::default()
                        },
                    },
                ],
                ..Default::default()
            }),
        );
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![ftd::Element::Row(ftd::Row {
                        spacing: None,
                        container: ftd::Container {
                            children: vec![
                                ftd::Element::Row(ftd::Row {
                                    spacing: None,
                                    common: ftd::Common {
                                        data_id: Some(s("r2")),
                                        id: Some(s("foo-1:r2")),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                }),
                                ftd::Element::Markup(ftd::Markups {
                                    text: ftd::markdown_line("hello"),
                                    line: true,
                                    ..Default::default()
                                }),
                            ],
                            ..Default::default()
                        },
                        common: ftd::Common {
                            data_id: Some(s("r1")),
                            id: Some(s("foo-1:r1")),
                            ..Default::default()
                        },
                    })],
                    ..Default::default()
                },
                common: ftd::Common {
                    data_id: Some(s("foo-1")),
                    id: Some(s("foo-1")),
                    ..Default::default()
                },
            }));

        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![ftd::Element::Row(ftd::Row {
                        spacing: None,
                        container: ftd::Container {
                            children: vec![ftd::Element::Row(ftd::Row {
                                spacing: None,
                                common: ftd::Common {
                                    data_id: Some(s("r2")),
                                    id: Some(s("foo-2:r2")),
                                    ..Default::default()
                                },
                                ..Default::default()
                            })],
                            ..Default::default()
                        },
                        common: ftd::Common {
                            data_id: Some(s("r1")),
                            id: Some(s("foo-2:r1")),
                            ..Default::default()
                        },
                    })],
                    ..Default::default()
                },
                common: ftd::Common {
                    data_id: Some(s("foo-2")),
                    id: Some(s("foo-2")),
                    ..Default::default()
                },
            }));

        p!(
            "
            -- ftd.column foo:

            --- ftd.row:
            id: r1

            --- ftd.row:
            id: r2

            -- foo:
            id: foo-1

            -- foo:
            id: foo-2

            -- container: foo-1.r1

            -- ftd.text: hello
            ",
            (bag, main),
        );
    }

    #[test]
    fn inner_container_using_import() {
        let mut bag = super::default_bag();

        bag.insert(
            "inner_container#foo".to_string(),
            ftd::p2::Thing::Component(ftd::Component {
                root: "ftd.column".to_string(),
                full_name: "inner_container#foo".to_string(),
                instructions: vec![
                    ftd::component::Instruction::ChildComponent {
                        child: ftd::component::ChildComponent {
                            is_recursive: false,
                            events: vec![],
                            root: "ftd#row".to_string(),
                            condition: None,
                            properties: std::array::IntoIter::new([(
                                s("id"),
                                ftd::component::Property {
                                    default: Some(ftd::PropertyValue::Value {
                                        value: ftd::variable::Value::String {
                                            text: "r1".to_string(),
                                            source: ftd::TextSource::Header,
                                        },
                                    }),
                                    conditions: vec![],
                                    ..Default::default()
                                },
                            )])
                            .collect(),
                            ..Default::default()
                        },
                    },
                    ftd::component::Instruction::ChildComponent {
                        child: ftd::component::ChildComponent {
                            is_recursive: false,
                            events: vec![],
                            root: "ftd#row".to_string(),
                            condition: None,
                            properties: std::array::IntoIter::new([(
                                s("id"),
                                ftd::component::Property {
                                    default: Some(ftd::PropertyValue::Value {
                                        value: ftd::variable::Value::String {
                                            text: "r2".to_string(),
                                            source: ftd::TextSource::Header,
                                        },
                                    }),
                                    conditions: vec![],
                                    ..Default::default()
                                },
                            )])
                            .collect(),
                            ..Default::default()
                        },
                    },
                ],
                ..Default::default()
            }),
        );
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![ftd::Element::Row(ftd::Row {
                        spacing: None,
                        container: ftd::Container {
                            children: vec![
                                ftd::Element::Row(ftd::Row {
                                    spacing: None,
                                    common: ftd::Common {
                                        data_id: Some(s("r2")),
                                        id: Some(s("foo-1:r2")),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                }),
                                ftd::Element::Markup(ftd::Markups {
                                    text: ftd::markdown_line("hello"),
                                    line: true,
                                    ..Default::default()
                                }),
                            ],
                            ..Default::default()
                        },
                        common: ftd::Common {
                            data_id: Some(s("r1")),
                            id: Some(s("foo-1:r1")),
                            ..Default::default()
                        },
                    })],
                    ..Default::default()
                },
                common: ftd::Common {
                    data_id: Some(s("foo-1")),
                    id: Some(s("foo-1")),
                    ..Default::default()
                },
            }));

        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![ftd::Element::Row(ftd::Row {
                        spacing: None,
                        container: ftd::Container {
                            children: vec![ftd::Element::Row(ftd::Row {
                                spacing: None,
                                common: ftd::Common {
                                    data_id: Some(s("r2")),
                                    id: Some(s("foo-2:r2")),
                                    ..Default::default()
                                },
                                ..Default::default()
                            })],
                            ..Default::default()
                        },
                        common: ftd::Common {
                            data_id: Some(s("r1")),
                            id: Some(s("foo-2:r1")),
                            ..Default::default()
                        },
                    })],
                    ..Default::default()
                },
                common: ftd::Common {
                    data_id: Some(s("foo-2")),
                    id: Some(s("foo-2")),
                    ..Default::default()
                },
            }));

        p!(
            "
            -- import: inner_container as ic

            -- ic.foo:
            id: foo-1

            -- ic.foo:
            id: foo-2

            -- container: foo-1.r1

            -- ftd.text: hello
            ",
            (bag, main),
        );
    }

    #[test]
    fn open_container_with_id() {
        let mut external_children = super::default_column();
        external_children.container.children = vec![ftd::Element::Markup(ftd::Markups {
            text: ftd::markdown_line("hello"),
            line: true,
            ..Default::default()
        })];

        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    external_children: Some((
                        s("some-child"),
                        vec![vec![0, 0]],
                        vec![ftd::Element::Column(external_children)],
                    )),
                    children: vec![ftd::Element::Row(ftd::Row {
                        spacing: None,
                        container: ftd::Container {
                            children: vec![ftd::Element::Row(ftd::Row {
                                spacing: None,
                                common: ftd::Common {
                                    data_id: Some(s("some-child")),
                                    ..Default::default()
                                },
                                ..Default::default()
                            })],
                            ..Default::default()
                        },
                        ..Default::default()
                    })],
                    open: (None, Some(s("some-child"))),
                    ..Default::default()
                },
                ..Default::default()
            }));

        let mut bag = super::default_bag();

        bag.insert(
            "foo/bar#foo".to_string(),
            ftd::p2::Thing::Component(ftd::Component {
                root: "ftd.column".to_string(),
                full_name: s("foo/bar#foo"),
                properties: std::array::IntoIter::new([(
                    s("open"),
                    ftd::component::Property {
                        default: Some(ftd::PropertyValue::Value {
                            value: ftd::Value::String {
                                text: s("some-child"),
                                source: ftd::TextSource::Header,
                            },
                        }),
                        conditions: vec![],
                        ..Default::default()
                    },
                )])
                .collect(),
                instructions: vec![
                    ftd::component::Instruction::ChildComponent {
                        child: ftd::component::ChildComponent {
                            events: vec![],
                            root: "ftd#row".to_string(),
                            condition: None,
                            ..Default::default()
                        },
                    },
                    ftd::component::Instruction::ChildComponent {
                        child: ftd::component::ChildComponent {
                            is_recursive: false,
                            events: vec![],
                            root: "ftd#row".to_string(),
                            condition: None,
                            properties: std::array::IntoIter::new([(
                                s("id"),
                                ftd::component::Property {
                                    default: Some(ftd::PropertyValue::Value {
                                        value: ftd::variable::Value::String {
                                            text: "some-child".to_string(),
                                            source: ftd::TextSource::Header,
                                        },
                                    }),
                                    conditions: vec![],
                                    ..Default::default()
                                },
                            )])
                            .collect(),
                            ..Default::default()
                        },
                    },
                ],
                ..Default::default()
            }),
        );

        p!(
            "
            -- ftd.column foo:
            open: some-child

            --- ftd.row:

            --- ftd.row:
            id: some-child

            -- foo:

            -- ftd.text: hello
            ",
            (bag, main),
        );
    }

    #[test]
    fn open_container_with_if() {
        let mut external_children = super::default_column();
        external_children.container.children = vec![
            ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("hello"),
                line: true,
                ..Default::default()
            }),
            ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("hello1"),
                line: true,
                ..Default::default()
            }),
        ];

        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("Start Browser"),
                line: true,
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![ftd::Element::Column(ftd::Column {
                        spacing: None,
                        container: ftd::Container {
                            children: vec![ftd::Element::Column(ftd::Column {
                                spacing: None,
                                container: ftd::Container {
                                    children: vec![
                                        ftd::Element::Column(ftd::Column {
                                            spacing: None,
                                            container: ftd::Container {
                                                children: vec![ftd::Element::Markup(
                                                    ftd::Markups {
                                                        text: ftd::markdown_line("Mobile Display"),
                                                        line: true,
                                                        common: ftd::Common {
                                                            data_id: Some(s("mobile-display")),
                                                            id: Some(s(
                                                                "foo-id:some-child:mobile-display",
                                                            )),
                                                            ..Default::default()
                                                        },
                                                        ..Default::default()
                                                    },
                                                )],
                                                ..Default::default()
                                            },
                                            common: ftd::Common {
                                                locals: std::array::IntoIter::new([(
                                                    s("id@1,0,0,0"),
                                                    s("some-child"),
                                                )])
                                                .collect(),
                                                condition: Some(ftd::Condition {
                                                    variable: s("foo/bar#mobile"),
                                                    value: s("true"),
                                                }),
                                                data_id: Some(s("some-child")),
                                                id: Some(s("foo-id:some-child")),
                                                ..Default::default()
                                            },
                                        }),
                                        ftd::Element::Column(ftd::Column {
                                            spacing: None,
                                            container: ftd::Container {
                                                children: vec![ftd::Element::Markup(
                                                    ftd::Markups {
                                                        text: ftd::markdown_line("Desktop Display"),
                                                        line: true,
                                                        ..Default::default()
                                                    },
                                                )],
                                                ..Default::default()
                                            },
                                            common: ftd::Common {
                                                locals: std::array::IntoIter::new([(
                                                    s("id@1,0,0,1"),
                                                    s("some-child"),
                                                )])
                                                .collect(),
                                                condition: Some(ftd::Condition {
                                                    variable: s("foo/bar#mobile"),
                                                    value: s("false"),
                                                }),
                                                is_not_visible: true,
                                                data_id: Some(s("some-child")),
                                                id: Some(s("foo-id:some-child")),
                                                ..Default::default()
                                            },
                                        }),
                                    ],
                                    external_children: Some((
                                        s("some-child"),
                                        vec![vec![0], vec![1]],
                                        vec![ftd::Element::Column(external_children)],
                                    )),
                                    open: (None, Some(s("some-child"))),
                                    ..Default::default()
                                },
                                common: ftd::Common {
                                    /*locals: std::array::IntoIter::new([
                                        (s("id@1,0,0"), s("foo-id")),
                                    ])
                                    .collect(),*/
                                    id: Some(s("foo-id")),
                                    data_id: Some(s("foo-id")),
                                    ..Default::default()
                                },
                            })],
                            ..Default::default()
                        },
                        common: ftd::Common {
                            data_id: Some(s("c2")),
                            id: Some(s("c2")),
                            ..Default::default()
                        },
                    })],
                    ..Default::default()
                },
                common: ftd::Common {
                    data_id: Some(s("c1")),
                    id: Some(s("c1")),
                    ..Default::default()
                },
            }));

        let mut bag = super::default_bag();
        bag.insert(
            s("foo/bar#desktop-display"),
            ftd::p2::Thing::Component(ftd::Component {
                root: "ftd.column".to_string(),
                full_name: s("foo/bar#desktop-display"),
                arguments: std::array::IntoIter::new([(
                    s("id"),
                    ftd::p2::Kind::optional(ftd::p2::Kind::string()),
                )])
                .collect(),
                properties: std::array::IntoIter::new([(
                    s("id"),
                    ftd::component::Property {
                        default: Some(ftd::PropertyValue::Variable {
                            name: "id".to_string(),
                            kind: ftd::p2::Kind::Optional {
                                kind: Box::new(ftd::p2::Kind::string()),
                            },
                        }),
                        conditions: vec![],
                        ..Default::default()
                    },
                )])
                .collect(),
                instructions: vec![ftd::component::Instruction::ChildComponent {
                    child: ftd::component::ChildComponent {
                        is_recursive: false,
                        events: vec![],
                        root: "ftd#text".to_string(),
                        condition: None,
                        properties: std::array::IntoIter::new([(
                            s("text"),
                            ftd::component::Property {
                                default: Some(ftd::PropertyValue::Value {
                                    value: ftd::variable::Value::String {
                                        text: s("Desktop Display"),
                                        source: ftd::TextSource::Caption,
                                    },
                                }),
                                conditions: vec![],
                                ..Default::default()
                            },
                        )])
                        .collect(),
                        ..Default::default()
                    },
                }],
                ..Default::default()
            }),
        );

        bag.insert(
            s("foo/bar#foo"),
            ftd::p2::Thing::Component(ftd::Component {
                root: "ftd.column".to_string(),
                full_name: s("foo/bar#foo"),
                properties: std::array::IntoIter::new([(
                    s("open"),
                    ftd::component::Property {
                        default: Some(ftd::PropertyValue::Value {
                            value: ftd::variable::Value::String {
                                text: s("some-child"),
                                source: ftd::TextSource::Header,
                            },
                        }),
                        conditions: vec![],
                        ..Default::default()
                    },
                )])
                .collect(),
                instructions: vec![
                    ftd::component::Instruction::ChildComponent {
                        child: ftd::component::ChildComponent {
                            is_recursive: false,
                            events: vec![],
                            root: "foo/bar#mobile-display".to_string(),
                            condition: Some(ftd::p2::Boolean::Equal {
                                left: ftd::PropertyValue::Reference {
                                    name: s("foo/bar#mobile"),
                                    kind: ftd::p2::Kind::Boolean { default: None },
                                },
                                right: ftd::PropertyValue::Value {
                                    value: ftd::variable::Value::Boolean { value: true },
                                },
                            }),
                            properties: std::array::IntoIter::new([(
                                s("id"),
                                ftd::component::Property {
                                    default: Some(ftd::PropertyValue::Value {
                                        value: ftd::variable::Value::String {
                                            text: s("some-child"),
                                            source: ftd::TextSource::Header,
                                        },
                                    }),
                                    conditions: vec![],
                                    ..Default::default()
                                },
                            )])
                            .collect(),
                            ..Default::default()
                        },
                    },
                    ftd::component::Instruction::ChildComponent {
                        child: ftd::component::ChildComponent {
                            is_recursive: false,
                            events: vec![],
                            root: "foo/bar#desktop-display".to_string(),
                            condition: Some(ftd::p2::Boolean::Equal {
                                left: ftd::PropertyValue::Reference {
                                    name: s("foo/bar#mobile"),
                                    kind: ftd::p2::Kind::Boolean { default: None },
                                },
                                right: ftd::PropertyValue::Value {
                                    value: ftd::variable::Value::Boolean { value: false },
                                },
                            }),
                            properties: std::array::IntoIter::new([(
                                s("id"),
                                ftd::component::Property {
                                    default: Some(ftd::PropertyValue::Value {
                                        value: ftd::variable::Value::String {
                                            text: s("some-child"),
                                            source: ftd::TextSource::Header,
                                        },
                                    }),
                                    conditions: vec![],
                                    ..Default::default()
                                },
                            )])
                            .collect(),
                            ..Default::default()
                        },
                    },
                ],
                ..Default::default()
            }),
        );

        bag.insert(
            s("foo/bar#mobile"),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: s("mobile"),
                value: ftd::PropertyValue::Value {
                    value: ftd::variable::Value::Boolean { value: true },
                },
                conditions: vec![],
            }),
        );

        bag.insert(
            s("foo/bar#mobile-display"),
            ftd::p2::Thing::Component(ftd::Component {
                root: "ftd.column".to_string(),
                full_name: s("foo/bar#mobile-display"),
                arguments: std::array::IntoIter::new([(
                    s("id"),
                    ftd::p2::Kind::optional(ftd::p2::Kind::string()),
                )])
                .collect(),
                properties: std::array::IntoIter::new([(
                    s("id"),
                    ftd::component::Property {
                        default: Some(ftd::PropertyValue::Variable {
                            name: "id".to_string(),
                            kind: ftd::p2::Kind::Optional {
                                kind: Box::new(ftd::p2::Kind::string()),
                            },
                        }),
                        conditions: vec![],
                        ..Default::default()
                    },
                )])
                .collect(),
                instructions: vec![ftd::component::Instruction::ChildComponent {
                    child: ftd::component::ChildComponent {
                        is_recursive: false,
                        events: vec![],
                        root: "ftd#text".to_string(),
                        condition: None,
                        properties: std::array::IntoIter::new([
                            (
                                s("id"),
                                ftd::component::Property {
                                    default: Some(ftd::PropertyValue::Value {
                                        value: ftd::variable::Value::String {
                                            text: s("mobile-display"),
                                            source: ftd::TextSource::Header,
                                        },
                                    }),
                                    conditions: vec![],
                                    ..Default::default()
                                },
                            ),
                            (
                                s("text"),
                                ftd::component::Property {
                                    default: Some(ftd::PropertyValue::Value {
                                        value: ftd::variable::Value::String {
                                            text: s("Mobile Display"),
                                            source: ftd::TextSource::Caption,
                                        },
                                    }),
                                    conditions: vec![],
                                    ..Default::default()
                                },
                            ),
                        ])
                        .collect(),
                        ..Default::default()
                    },
                }],
                ..Default::default()
            }),
        );

        p!(
            "
            -- ftd.column mobile-display:
            optional string id:
            id: $id

            --- ftd.text: Mobile Display
            id: mobile-display

            -- ftd.column desktop-display:
            optional string id:
            id: $id

            --- ftd.text: Desktop Display

            -- boolean mobile: true

            -- ftd.column foo:
            open: some-child

            --- mobile-display:
            if: $mobile
            id: some-child

            --- desktop-display:
            if: not $mobile
            id: some-child

            -- ftd.text: Start Browser

            -- ftd.column:
            id: c1

            -- ftd.column:
            id: c2

            -- foo:
            id: foo-id

            -- ftd.text: hello

            -- ftd.text: hello1
            ",
            (bag, main),
        );
    }

    #[test]
    fn nested_open_container() {
        let mut external_children = super::default_column();
        external_children.container.children = vec![
            ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("hello"),
                line: true,
                ..Default::default()
            }),
            ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("hello again"),
                line: true,
                ..Default::default()
            }),
        ];

        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![ftd::Element::Column(ftd::Column {
                        spacing: None,
                        container: ftd::Container {
                            children: vec![
                                ftd::Element::Column(ftd::Column {
                                    spacing: None,
                                    container: ftd::Container {
                                        children: vec![ftd::Element::Column(ftd::Column {
                                            spacing: None,
                                            container: ftd::Container {
                                                children: vec![],
                                                ..Default::default()
                                            },
                                            common: ftd::Common {
                                                data_id: Some(s("desktop-container")),
                                                ..Default::default()
                                            },
                                        })],
                                        external_children: Some((
                                            s("desktop-container"),
                                            vec![vec![0]],
                                            vec![],
                                        )),
                                        open: (None, Some(s("desktop-container"))),
                                        ..Default::default()
                                    },
                                    common: ftd::Common {
                                        condition: Some(ftd::Condition {
                                            variable: s("foo/bar#is-mobile"),
                                            value: s("false"),
                                        }),
                                        is_not_visible: true,
                                        data_id: Some(s("main-container")),
                                        ..Default::default()
                                    },
                                }),
                                ftd::Element::Column(ftd::Column {
                                    spacing: None,
                                    container: ftd::Container {
                                        children: vec![ftd::Element::Column(ftd::Column {
                                            spacing: None,
                                            common: ftd::Common {
                                                data_id: Some(s("mobile-container")),
                                                ..Default::default()
                                            },
                                            ..Default::default()
                                        })],
                                        external_children: Some((
                                            s("mobile-container"),
                                            vec![vec![0]],
                                            vec![],
                                        )),
                                        open: (None, Some(s("mobile-container"))),
                                        ..Default::default()
                                    },
                                    common: ftd::Common {
                                        condition: Some(ftd::Condition {
                                            variable: s("foo/bar#is-mobile"),
                                            value: s("true"),
                                        }),
                                        data_id: Some(s("main-container")),
                                        ..Default::default()
                                    },
                                }),
                            ],
                            ..Default::default()
                        },
                        common: ftd::Common {
                            data_id: Some(s("start")),
                            ..Default::default()
                        },
                    })],
                    external_children: Some((
                        s("main-container"),
                        vec![vec![0, 0], vec![0, 1]],
                        vec![ftd::Element::Column(external_children)],
                    )),
                    open: (None, Some(s("main-container"))),
                    ..Default::default()
                },
                ..Default::default()
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- ftd.column desktop:
                open: desktop-container

                --- ftd.column:
                id: desktop-container

                -- ftd.column mobile:
                open: mobile-container

                --- ftd.column:
                id: mobile-container

                -- boolean is-mobile: true

                -- ftd.column page:
                open: main-container

                --- ftd.column:
                id: start

                --- desktop:
                if: not $is-mobile
                id: main-container

                --- container: start

                --- mobile:
                if: $is-mobile
                id: main-container

                -- page:

                -- ftd.text: hello

                -- ftd.text: hello again
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn deep_open_container_call() {
        let mut external_children = super::default_column();
        external_children.container.children = vec![
            ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("hello"),
                line: true,
                ..Default::default()
            }),
            ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("hello again"),
                line: true,
                ..Default::default()
            }),
        ];

        let mut main = super::default_column();

        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![
                        ftd::Element::Column(ftd::Column {
                            spacing: None,
                            container: ftd::Container {
                                children: vec![ftd::Element::Column(ftd::Column {
                                    spacing: None,
                                    common: ftd::Common {
                                        data_id: Some(s("foo")),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                })],
                                ..Default::default()
                            },
                            common: ftd::Common {
                                locals: std::array::IntoIter::new([(
                                    s("id@0,0"),
                                    s("main-container"),
                                )])
                                .collect(),
                                condition: Some(ftd::Condition {
                                    variable: s("foo/bar#is-mobile"),
                                    value: s("false"),
                                }),
                                is_not_visible: true,
                                data_id: Some(s("main-container")),
                                ..Default::default()
                            },
                        }),
                        ftd::Element::Column(ftd::Column {
                            spacing: None,
                            container: ftd::Container {
                                children: vec![ftd::Element::Column(ftd::Column {
                                    spacing: None,
                                    common: ftd::Common {
                                        data_id: Some(s("foo")),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                })],
                                ..Default::default()
                            },
                            common: ftd::Common {
                                locals: std::array::IntoIter::new([(
                                    s("id@0,1"),
                                    s("main-container"),
                                )])
                                .collect(),
                                condition: Some(ftd::Condition {
                                    variable: s("foo/bar#is-mobile"),
                                    value: s("true"),
                                }),
                                data_id: Some(s("main-container")),
                                ..Default::default()
                            },
                        }),
                    ],
                    external_children: Some((
                        s("foo"),
                        vec![vec![0, 0], vec![1, 0]],
                        vec![ftd::Element::Column(external_children)],
                    )),
                    open: (None, Some(s("main-container.foo"))),
                    ..Default::default()
                },
                ..Default::default()
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- ftd.column desktop:
                optional string id:
                id: $id

                --- ftd.column:
                id: foo

                -- ftd.column mobile:
                optional string id:
                id: $id

                --- ftd.column:
                id: foo

                -- boolean is-mobile: true

                -- ftd.column page:
                open: main-container.foo

                --- desktop:
                if: not $is-mobile
                id: main-container

                --- mobile:
                if: $is-mobile
                id: main-container

                -- page:

                -- ftd.text: hello

                -- ftd.text: hello again
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn deep_nested_open_container_call() {
        let mut nested_external_children = super::default_column();
        nested_external_children.container.children = vec![
            ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("hello"),
                line: true,
                ..Default::default()
            }),
            ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("hello again"),
                line: true,
                ..Default::default()
            }),
        ];

        let mut external_children = super::default_column();
        external_children.container.children = vec![ftd::Element::Column(ftd::Column {
            spacing: None,
            container: ftd::Container {
                children: vec![ftd::Element::Row(ftd::Row {
                    spacing: None,
                    container: ftd::Container {
                        children: vec![ftd::Element::Column(ftd::Column {
                            spacing: None,
                            common: ftd::Common {
                                locals: std::array::IntoIter::new([(s("id@0,0,0,0"), s("foo"))])
                                    .collect(),
                                data_id: Some(s("foo")),
                                ..Default::default()
                            },
                            ..Default::default()
                        })],
                        ..Default::default()
                    },
                    common: ftd::Common {
                        data_id: Some(s("desktop-container")),
                        ..Default::default()
                    },
                })],
                external_children: Some((
                    s("desktop-container"),
                    vec![vec![0]],
                    vec![ftd::Element::Column(nested_external_children)],
                )),
                open: (None, Some(s("desktop-container"))),
                ..Default::default()
            },
            common: ftd::Common {
                locals: std::array::IntoIter::new([(s("id@0,0"), s(""))]).collect(),
                ..Default::default()
            },
            ..Default::default()
        })];

        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![
                        ftd::Element::Column(ftd::Column {
                            spacing: None,
                            container: ftd::Container {
                                children: vec![ftd::Element::Row(ftd::Row {
                                    spacing: None,
                                    container: ftd::Container {
                                        children: vec![ftd::Element::Column(ftd::Column {
                                            spacing: None,
                                            common: ftd::Common {
                                                locals: std::array::IntoIter::new([(
                                                    s("id@0,0,0,0"),
                                                    s("foo"),
                                                )])
                                                .collect(),
                                                data_id: Some(s("foo")),
                                                ..Default::default()
                                            },
                                            ..Default::default()
                                        })],
                                        ..Default::default()
                                    },
                                    common: ftd::Common {
                                        data_id: Some(s("desktop-container")),
                                        ..Default::default()
                                    },
                                })],
                                external_children: Some((
                                    s("desktop-container"),
                                    vec![vec![0]],
                                    vec![],
                                )),
                                open: (None, Some(s("desktop-container"))),
                                ..Default::default()
                            },
                            common: ftd::Common {
                                locals: std::array::IntoIter::new([(
                                    s("id@0,0"),
                                    s("main-container"),
                                )])
                                .collect(),
                                condition: Some(ftd::Condition {
                                    variable: s("foo/bar#is-mobile"),
                                    value: s("false"),
                                }),
                                data_id: Some(s("main-container")),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                        ftd::Element::Column(ftd::Column {
                            spacing: None,
                            container: ftd::Container {
                                children: vec![ftd::Element::Row(ftd::Row {
                                    spacing: None,
                                    container: ftd::Container {
                                        children: vec![ftd::Element::Column(ftd::Column {
                                            spacing: None,
                                            common: ftd::Common {
                                                locals: std::array::IntoIter::new([(
                                                    s("id@0,1,0,0"),
                                                    s("foo"),
                                                )])
                                                .collect(),
                                                data_id: Some(s("foo")),
                                                ..Default::default()
                                            },
                                            ..Default::default()
                                        })],
                                        ..Default::default()
                                    },
                                    common: ftd::Common {
                                        data_id: Some(s("mobile-container")),
                                        ..Default::default()
                                    },
                                })],
                                external_children: Some((
                                    s("mobile-container"),
                                    vec![vec![0]],
                                    vec![],
                                )),
                                open: (None, Some(s("mobile-container"))),
                                ..Default::default()
                            },
                            common: ftd::Common {
                                locals: std::array::IntoIter::new([(
                                    s("id@0,1"),
                                    s("main-container"),
                                )])
                                .collect(),
                                condition: Some(ftd::Condition {
                                    variable: s("foo/bar#is-mobile"),
                                    value: s("true"),
                                }),
                                is_not_visible: true,
                                data_id: Some(s("main-container")),
                                ..Default::default()
                            },
                        }),
                    ],
                    external_children: Some((
                        s("foo"),
                        vec![vec![0, 0, 0], vec![1, 0, 0]],
                        vec![ftd::Element::Column(external_children)],
                    )),
                    open: (None, Some(s("main-container.foo"))),
                    ..Default::default()
                },
                ..Default::default()
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- ftd.column ft_container:
                optional string id:
                id: $id

                -- ftd.column ft_container_mobile:
                optional string id:
                id: $id


                -- ftd.column desktop:
                open: desktop-container
                optional string id:
                id: $id

                --- ftd.row:
                id: desktop-container

                --- ft_container:
                id: foo



                -- ftd.column mobile:
                open: mobile-container
                optional string id:
                id: $id

                --- ftd.row:
                id: mobile-container

                --- ft_container_mobile:
                id: foo


                -- boolean is-mobile: false


                -- ftd.column page:
                open: main-container.foo

                --- desktop:
                if: not $is-mobile
                id: main-container

                --- container: ftd.main

                --- mobile:
                if: $is-mobile
                id: main-container



                -- page:

                -- desktop:

                -- ftd.text: hello

                -- ftd.text: hello again

                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn invalid_deep_open_container() {
        let mut external_children = super::default_column();
        external_children.container.children = vec![
            ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("hello"),
                line: true,
                ..Default::default()
            }),
            ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("hello again"),
                line: true,
                ..Default::default()
            }),
        ];

        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![ftd::Element::Column(ftd::Column {
                        spacing: None,
                        container: ftd::Container {
                            children: vec![
                                ftd::Element::Column(ftd::Column {
                                    spacing: None,
                                    container: ftd::Container {
                                        children: vec![ftd::Element::Column(ftd::Column {
                                            spacing: None,
                                            container: ftd::Container {
                                                children: vec![],
                                                ..Default::default()
                                            },
                                            common: ftd::Common {
                                                data_id: Some(s("main-container")),
                                                ..Default::default()
                                            },
                                        })],
                                        ..Default::default()
                                    },
                                    common: ftd::Common {
                                        locals: std::array::IntoIter::new([(s("id@0,0,0"), s(""))])
                                            .collect(),
                                        condition: Some(ftd::Condition {
                                            variable: s("foo/bar#is-mobile"),
                                            value: s("false"),
                                        }),
                                        is_not_visible: true,
                                        ..Default::default()
                                    },
                                }),
                                ftd::Element::Column(ftd::Column {
                                    spacing: None,
                                    container: ftd::Container {
                                        children: vec![ftd::Element::Column(ftd::Column {
                                            spacing: None,
                                            common: ftd::Common {
                                                data_id: Some(s("main-container")),
                                                ..Default::default()
                                            },
                                            ..Default::default()
                                        })],
                                        ..Default::default()
                                    },
                                    common: ftd::Common {
                                        locals: std::array::IntoIter::new([(s("id@0,0,1"), s(""))])
                                            .collect(),
                                        condition: Some(ftd::Condition {
                                            variable: s("foo/bar#is-mobile"),
                                            value: s("true"),
                                        }),
                                        ..Default::default()
                                    },
                                }),
                            ],
                            ..Default::default()
                        },
                        common: ftd::Common {
                            data_id: Some(s("start")),
                            ..Default::default()
                        },
                    })],
                    external_children: Some((
                        s("main-container"),
                        vec![],
                        vec![ftd::Element::Column(external_children)],
                    )),
                    open: (None, Some(s("main-container"))),
                    ..Default::default()
                },
                ..Default::default()
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- ftd.column desktop:
                optional string id:
                id: $id

                --- ftd.column:
                id: main-container

                -- ftd.column mobile:
                optional string id:
                id: $id

                --- ftd.column:
                id: main-container

                -- boolean is-mobile: true

                -- ftd.column page:
                open: main-container

                --- ftd.column:
                id: start

                --- desktop:
                if: not $is-mobile

                --- container: start

                --- mobile:
                if: $is-mobile

                -- page:

                -- ftd.text: hello

                -- ftd.text: hello again
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn open_container_id_1() {
        let mut main = self::default_column();
        main.container.children.push(ftd::Element::Row(ftd::Row {
            spacing: None,
            common: ftd::Common {
                data_id: Some(s("r1")),
                id: Some(s("r1")),
                ..Default::default()
            },
            container: ftd::Container {
                open: (Some(false), None),
                ..Default::default()
            },
        }));
        main.container.children.push(ftd::Element::Row(ftd::Row {
            spacing: None,
            container: ftd::Container {
                external_children: Default::default(),
                children: vec![
                    ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("hello"),
                        line: true,
                        ..Default::default()
                    }),
                    ftd::Element::Row(ftd::Row {
                        spacing: None,
                        container: ftd::Container {
                            open: (Some(false), None),
                            ..Default::default()
                        },
                        common: ftd::Common {
                            data_id: Some(s("r3")),
                            id: Some(s("r3")),
                            ..Default::default()
                        },
                    }),
                ],
                open: (Some(true), None),
                ..Default::default()
            },
            common: ftd::Common {
                data_id: Some(s("r2")),
                id: Some(s("r2")),
                ..Default::default()
            },
        }));
        let (g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- ftd.row:
                id: r1
                open: false

                -- ftd.row:
                id: r2
                open: true

                --- ftd.text: hello

                -- ftd.row:
                id: r3
                open: false
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");
        pretty_assertions::assert_eq!(g_bag, super::default_bag());
        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn submit() {
        let mut main = super::default_column();

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("hello"),
                line: true,
                common: ftd::Common {
                    submit: Some("https://httpbin.org/post?x=10".to_string()),
                    ..Default::default()
                },
                ..Default::default()
            }));

        let (g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- ftd.text: hello
                submit: https://httpbin.org/post?x=10
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");
        pretty_assertions::assert_eq!(g_bag, super::default_bag());
        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn basic_loop_on_record_1() {
        let mut main = super::default_column();
        main.container.children.push(ftd::Element::Row(ftd::Row {
            spacing: None,
            container: ftd::Container {
                children: vec![
                    ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("hello"),
                        line: true,
                        common: ftd::Common {
                            reference: Some(s("@name@0")),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                    ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("world"),
                        line: true,
                        common: ftd::Common {
                            reference: Some(s("@body@0")),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                ],
                ..Default::default()
            },
            common: ftd::Common {
                locals: std::array::IntoIter::new([
                    (s("body@0"), s("world")),
                    (s("name@0"), s("hello")),
                ])
                .collect(),
                ..Default::default()
            },
        }));

        main.container.children.push(ftd::Element::Row(ftd::Row {
            spacing: None,
            container: ftd::Container {
                children: vec![
                    ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("Arpita Jaiswal"),
                        line: true,
                        common: ftd::Common {
                            reference: Some(s("@name@1")),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                    ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("Arpita is developer at Fifthtry"),
                        line: true,
                        common: ftd::Common {
                            reference: Some(s("@body@1")),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                ],
                ..Default::default()
            },
            common: ftd::Common {
                locals: std::array::IntoIter::new([
                    (s("body@1"), s("Arpita is developer at Fifthtry")),
                    (s("name@1"), s("Arpita Jaiswal")),
                ])
                .collect(),
                reference: Some(s("foo/bar#people")),
                ..Default::default()
            },
        }));

        main.container.children.push(ftd::Element::Row(ftd::Row {
            spacing: None,
            container: ftd::Container {
                children: vec![
                    ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("Amit Upadhyay"),
                        line: true,
                        common: ftd::Common {
                            reference: Some(s("@name@2")),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                    ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("Amit is CEO of FifthTry."),
                        line: true,
                        common: ftd::Common {
                            reference: Some(s("@body@2")),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                ],
                ..Default::default()
            },
            common: ftd::Common {
                locals: std::array::IntoIter::new([
                    (s("body@2"), s("Amit is CEO of FifthTry.")),
                    (s("name@2"), s("Amit Upadhyay")),
                ])
                .collect(),
                reference: Some(s("foo/bar#people")),
                ..Default::default()
            },
        }));

        let mut bag = super::default_bag();

        bag.insert(
            "foo/bar#foo".to_string(),
            ftd::p2::Thing::Component(ftd::Component {
                root: "ftd.row".to_string(),
                full_name: s("foo/bar#foo"),
                arguments: std::array::IntoIter::new([
                    (s("body"), ftd::p2::Kind::string()),
                    (s("name"), ftd::p2::Kind::caption()),
                ])
                .collect(),
                instructions: vec![
                    ftd::component::Instruction::ChildComponent {
                        child: ftd::component::ChildComponent {
                            is_recursive: false,
                            events: vec![],
                            root: "ftd#text".to_string(),
                            condition: None,
                            properties: std::array::IntoIter::new([(
                                s("text"),
                                ftd::component::Property {
                                    default: Some(ftd::PropertyValue::Variable {
                                        name: "name".to_string(),
                                        kind: ftd::p2::Kind::caption_or_body(),
                                    }),
                                    conditions: vec![],
                                    ..Default::default()
                                },
                            )])
                            .collect(),
                            ..Default::default()
                        },
                    },
                    ftd::component::Instruction::ChildComponent {
                        child: ftd::component::ChildComponent {
                            is_recursive: false,
                            events: vec![],
                            root: "ftd#text".to_string(),
                            condition: None,
                            properties: std::array::IntoIter::new([(
                                s("text"),
                                ftd::component::Property {
                                    default: Some(ftd::PropertyValue::Variable {
                                        name: "body".to_string(),
                                        kind: ftd::p2::Kind::caption_or_body(),
                                    }),
                                    conditions: vec![],
                                    ..Default::default()
                                },
                            )])
                            .collect(),
                            ..Default::default()
                        },
                    },
                ],
                ..Default::default()
            }),
        );

        bag.insert(
            "foo/bar#get".to_string(),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: "get".to_string(),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::String {
                        text: "world".to_string(),
                        source: ftd::TextSource::Caption,
                    },
                },
                conditions: vec![],
            }),
        );

        bag.insert(
            "foo/bar#name".to_string(),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: "name".to_string(),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::String {
                        text: "Arpita Jaiswal".to_string(),
                        source: ftd::TextSource::Caption,
                    },
                },
                conditions: vec![],
            }),
        );

        bag.insert(
            "foo/bar#people".to_string(),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: "foo/bar#people".to_string(),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::List {
                        data: vec![
                            ftd::PropertyValue::Value {
                                value: ftd::Value::Record {
                                    name: "foo/bar#person".to_string(),
                                    fields: std::array::IntoIter::new([
                                        (
                                            s("bio"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::Value::String {
                                                    text: "Arpita is developer at Fifthtry"
                                                        .to_string(),
                                                    source: ftd::TextSource::Body,
                                                },
                                            },
                                        ),
                                        (
                                            s("name"),
                                            ftd::PropertyValue::Reference {
                                                name: "foo/bar#name".to_string(),
                                                kind: ftd::p2::Kind::caption(),
                                            },
                                        ),
                                    ])
                                    .collect(),
                                },
                            },
                            ftd::PropertyValue::Value {
                                value: ftd::Value::Record {
                                    name: "foo/bar#person".to_string(),
                                    fields: std::array::IntoIter::new([
                                        (
                                            s("bio"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::Value::String {
                                                    text: "Amit is CEO of FifthTry.".to_string(),
                                                    source: ftd::TextSource::Body,
                                                },
                                            },
                                        ),
                                        (
                                            s("name"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::Value::String {
                                                    text: "Amit Upadhyay".to_string(),
                                                    source: ftd::TextSource::Caption,
                                                },
                                            },
                                        ),
                                    ])
                                    .collect(),
                                },
                            },
                        ],
                        kind: ftd::p2::Kind::Record {
                            name: "foo/bar#person".to_string(),
                            default: None,
                        },
                    },
                },
                conditions: vec![],
            }),
        );

        bag.insert(
            "foo/bar#person".to_string(),
            ftd::p2::Thing::Record(ftd::p2::Record {
                name: "foo/bar#person".to_string(),
                fields: std::array::IntoIter::new([
                    (s("bio"), ftd::p2::Kind::body()),
                    (s("name"), ftd::p2::Kind::caption()),
                ])
                .collect(),
                instances: Default::default(),
                order: vec![s("name"), s("bio")],
            }),
        );

        p!(
            "
            -- ftd.row foo:
            caption name:
            string body:

            --- ftd.text: $name

            --- ftd.text: $body

            -- record person:
            caption name:
            body bio:

            -- person list people:

            -- string name: Arpita Jaiswal

            -- people: $name

            Arpita is developer at Fifthtry

            -- people: Amit Upadhyay

            Amit is CEO of FifthTry.

            -- string get: world

            -- foo: hello
            body: $get

            -- foo: $obj.name
            $loop$: $people as $obj
            body: $obj.bio
            ",
            (bag, main),
        );
    }

    #[test]
    fn basic_loop_on_record_with_if_condition() {
        let mut main = super::default_column();
        main.container.children.push(ftd::Element::Null);

        main.container.children.push(ftd::Element::Row(ftd::Row {
            spacing: None,
            container: ftd::Container {
                children: vec![
                    ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("Amit Upadhyay"),
                        line: true,
                        common: ftd::Common {
                            reference: Some(s("@name@1")),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                    ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("Amit is CEO of FifthTry."),
                        line: true,
                        common: ftd::Common {
                            reference: Some(s("@body@1")),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                ],
                ..Default::default()
            },
            common: ftd::Common {
                locals: std::array::IntoIter::new([
                    (s("name@1"), s("Amit Upadhyay")),
                    (s("body@1"), s("Amit is CEO of FifthTry.")),
                ])
                .collect(),
                reference: Some(s("foo/bar#people")),
                ..Default::default()
            },
            ..Default::default()
        }));

        let mut bag = super::default_bag();

        bag.insert(
            "foo/bar#foo".to_string(),
            ftd::p2::Thing::Component(ftd::Component {
                root: "ftd.row".to_string(),
                full_name: s("foo/bar#foo"),
                arguments: std::array::IntoIter::new([
                    (s("body"), ftd::p2::Kind::string()),
                    (s("name"), ftd::p2::Kind::caption()),
                ])
                .collect(),
                instructions: vec![
                    ftd::component::Instruction::ChildComponent {
                        child: ftd::component::ChildComponent {
                            is_recursive: false,
                            events: vec![],
                            root: "ftd#text".to_string(),
                            condition: None,
                            properties: std::array::IntoIter::new([(
                                s("text"),
                                ftd::component::Property {
                                    default: Some(ftd::PropertyValue::Variable {
                                        name: "name".to_string(),
                                        kind: ftd::p2::Kind::caption_or_body(),
                                    }),
                                    conditions: vec![],
                                    ..Default::default()
                                },
                            )])
                            .collect(),
                            ..Default::default()
                        },
                    },
                    ftd::component::Instruction::ChildComponent {
                        child: ftd::component::ChildComponent {
                            is_recursive: false,
                            events: vec![],
                            root: "ftd#text".to_string(),
                            condition: None,
                            properties: std::array::IntoIter::new([(
                                s("text"),
                                ftd::component::Property {
                                    default: Some(ftd::PropertyValue::Variable {
                                        name: "body".to_string(),
                                        kind: ftd::p2::Kind::caption_or_body(),
                                    }),
                                    conditions: vec![],
                                    ..Default::default()
                                },
                            )])
                            .collect(),
                            ..Default::default()
                        },
                    },
                ],
                ..Default::default()
            }),
        );

        bag.insert(
            "foo/bar#people".to_string(),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: "foo/bar#people".to_string(),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::List {
                        data: vec![
                            ftd::PropertyValue::Value {
                                value: ftd::Value::Record {
                                    name: "foo/bar#person".to_string(),
                                    fields: std::array::IntoIter::new([
                                        (
                                            s("bio"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::Value::String {
                                                    text: "Arpita is developer at Fifthtry"
                                                        .to_string(),
                                                    source: ftd::TextSource::Body,
                                                },
                                            },
                                        ),
                                        (
                                            s("ceo"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::Value::Boolean { value: false },
                                            },
                                        ),
                                        (
                                            s("name"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::Value::String {
                                                    text: "Arpita Jaiswal".to_string(),
                                                    source: ftd::TextSource::Caption,
                                                },
                                            },
                                        ),
                                    ])
                                    .collect(),
                                },
                            },
                            ftd::PropertyValue::Value {
                                value: ftd::Value::Record {
                                    name: "foo/bar#person".to_string(),
                                    fields: std::array::IntoIter::new([
                                        (
                                            s("bio"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::Value::String {
                                                    text: "Amit is CEO of FifthTry.".to_string(),
                                                    source: ftd::TextSource::Body,
                                                },
                                            },
                                        ),
                                        (
                                            s("ceo"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::Value::Boolean { value: true },
                                            },
                                        ),
                                        (
                                            s("name"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::Value::String {
                                                    text: "Amit Upadhyay".to_string(),
                                                    source: ftd::TextSource::Caption,
                                                },
                                            },
                                        ),
                                    ])
                                    .collect(),
                                },
                            },
                        ],
                        kind: ftd::p2::Kind::Record {
                            name: "foo/bar#person".to_string(),
                            default: None,
                        },
                    },
                },
                conditions: vec![],
            }),
        );

        bag.insert(
            "foo/bar#person".to_string(),
            ftd::p2::Thing::Record(ftd::p2::Record {
                name: "foo/bar#person".to_string(),
                fields: std::array::IntoIter::new([
                    (s("bio"), ftd::p2::Kind::body()),
                    (s("name"), ftd::p2::Kind::caption()),
                    (s("ceo"), ftd::p2::Kind::boolean()),
                ])
                .collect(),
                instances: Default::default(),
                order: vec![s("name"), s("bio"), s("ceo")],
            }),
        );

        p!(
            "
            -- ftd.row foo:
            caption name:
            string body:

            --- ftd.text: $name

            --- ftd.text: $body

            -- record person:
            caption name:
            body bio:
            boolean ceo:

            -- person list people:

            -- people: Arpita Jaiswal
            ceo: false

            Arpita is developer at Fifthtry

            -- people: Amit Upadhyay
            ceo: true

            Amit is CEO of FifthTry.

            -- foo: $obj.name
            $loop$: $people as $obj
            if: $obj.ceo
            body: $obj.bio
            ",
            (bag, main),
        );
    }

    #[test]
    fn basic_loop_on_string() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("Arpita"),
                line: true,
                common: ftd::Common {
                    reference: Some(s("foo/bar#people")),
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("Asit"),
                line: true,
                common: ftd::Common {
                    reference: Some(s("foo/bar#people")),
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("Sourabh"),
                line: true,
                common: ftd::Common {
                    reference: Some(s("foo/bar#people")),
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("$loop$"),
                line: true,
                common: ftd::Common {
                    reference: Some(s("foo/bar#people")),
                    is_dummy: true,
                    ..Default::default()
                },
                ..Default::default()
            }));

        let mut bag = super::default_bag();

        bag.insert(
            "foo/bar#people".to_string(),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: "foo/bar#people".to_string(),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::List {
                        data: vec![
                            ftd::PropertyValue::Value {
                                value: ftd::Value::String {
                                    text: "Arpita".to_string(),
                                    source: ftd::TextSource::Caption,
                                },
                            },
                            ftd::PropertyValue::Value {
                                value: ftd::Value::String {
                                    text: "Asit".to_string(),
                                    source: ftd::TextSource::Caption,
                                },
                            },
                            ftd::PropertyValue::Value {
                                value: ftd::Value::String {
                                    text: "Sourabh".to_string(),
                                    source: ftd::TextSource::Caption,
                                },
                            },
                        ],
                        kind: ftd::p2::Kind::string(),
                    },
                },
                conditions: vec![],
            }),
        );
        let (g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- string list people:

                -- people: Arpita

                -- people: Asit

                -- people: Sourabh

                -- ftd.text: $obj
                $loop$: $people as $obj
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");
        pretty_assertions::assert_eq!(g_bag, bag);
        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn loop_inside_subsection() {
        let mut main = super::default_column();
        let mut col = ftd::Column {
            ..Default::default()
        };

        col.container.children.push(ftd::Element::Row(ftd::Row {
            spacing: None,
            container: ftd::Container {
                children: vec![
                    ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("Arpita Jaiswal"),
                        line: true,
                        common: ftd::Common {
                            reference: Some(s("@name@0,0")),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                    ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("Arpita is developer at Fifthtry"),
                        line: true,
                        common: ftd::Common {
                            reference: Some(s("@body@0,0")),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                ],
                ..Default::default()
            },
            common: ftd::Common {
                locals: std::array::IntoIter::new([
                    (s("body@0,0"), s("Arpita is developer at Fifthtry")),
                    (s("name@0,0"), s("Arpita Jaiswal")),
                ])
                .collect(),
                reference: Some(s("foo/bar#people")),
                ..Default::default()
            },
        }));

        col.container.children.push(ftd::Element::Row(ftd::Row {
            spacing: None,
            container: ftd::Container {
                children: vec![
                    ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("Amit Upadhyay"),
                        line: true,
                        common: ftd::Common {
                            reference: Some(s("@name@0,1")),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                    ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("Amit is CEO of FifthTry."),
                        line: true,
                        common: ftd::Common {
                            reference: Some(s("@body@0,1")),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                ],
                ..Default::default()
            },
            common: ftd::Common {
                locals: std::array::IntoIter::new([
                    (s("body@0,1"), s("Amit is CEO of FifthTry.")),
                    (s("name@0,1"), s("Amit Upadhyay")),
                ])
                .collect(),
                reference: Some(s("foo/bar#people")),
                ..Default::default()
            },
        }));

        main.container.children.push(ftd::Element::Column(col));

        let mut bag = super::default_bag();

        bag.insert(
            "foo/bar#foo".to_string(),
            ftd::p2::Thing::Component(ftd::Component {
                root: "ftd.row".to_string(),
                full_name: s("foo/bar#foo"),
                arguments: std::array::IntoIter::new([
                    (s("body"), ftd::p2::Kind::string()),
                    (s("name"), ftd::p2::Kind::caption()),
                ])
                .collect(),
                instructions: vec![
                    ftd::component::Instruction::ChildComponent {
                        child: ftd::component::ChildComponent {
                            is_recursive: false,
                            events: vec![],
                            root: "ftd#text".to_string(),
                            condition: None,
                            properties: std::array::IntoIter::new([(
                                s("text"),
                                ftd::component::Property {
                                    default: Some(ftd::PropertyValue::Variable {
                                        name: "name".to_string(),
                                        kind: ftd::p2::Kind::caption_or_body(),
                                    }),
                                    conditions: vec![],
                                    ..Default::default()
                                },
                            )])
                            .collect(),
                            ..Default::default()
                        },
                    },
                    ftd::component::Instruction::ChildComponent {
                        child: ftd::component::ChildComponent {
                            is_recursive: false,
                            events: vec![],
                            root: "ftd#text".to_string(),
                            condition: None,
                            properties: std::array::IntoIter::new([(
                                s("text"),
                                ftd::component::Property {
                                    default: Some(ftd::PropertyValue::Variable {
                                        name: "body".to_string(),
                                        kind: ftd::p2::Kind::caption_or_body(),
                                    }),
                                    conditions: vec![],
                                    ..Default::default()
                                },
                            )])
                            .collect(),
                            ..Default::default()
                        },
                    },
                ],
                invocations: vec![
                    std::array::IntoIter::new([
                        (
                            s("body"),
                            ftd::Value::String {
                                text: s("Arpita is developer at Fifthtry"),
                                source: ftd::TextSource::Body,
                            },
                        ),
                        (
                            s("name"),
                            ftd::Value::String {
                                text: s("Arpita Jaiswal"),
                                source: ftd::TextSource::Caption,
                            },
                        ),
                    ])
                    .collect(),
                    std::array::IntoIter::new([
                        (
                            s("body"),
                            ftd::Value::String {
                                text: s("Amit is CEO of FifthTry."),
                                source: ftd::TextSource::Body,
                            },
                        ),
                        (
                            s("name"),
                            ftd::Value::String {
                                text: s("Amit Upadhyay"),
                                source: ftd::TextSource::Caption,
                            },
                        ),
                    ])
                    .collect(),
                ],
                ..Default::default()
            }),
        );

        bag.insert(
            "foo/bar#people".to_string(),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: "foo/bar#people".to_string(),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::List {
                        data: vec![
                            ftd::PropertyValue::Value {
                                value: ftd::Value::Record {
                                    name: "foo/bar#person".to_string(),
                                    fields: std::array::IntoIter::new([
                                        (
                                            s("bio"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::Value::String {
                                                    text: "Arpita is developer at Fifthtry"
                                                        .to_string(),
                                                    source: ftd::TextSource::Body,
                                                },
                                            },
                                        ),
                                        (
                                            s("name"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::Value::String {
                                                    text: "Arpita Jaiswal".to_string(),
                                                    source: ftd::TextSource::Caption,
                                                },
                                            },
                                        ),
                                    ])
                                    .collect(),
                                },
                            },
                            ftd::PropertyValue::Value {
                                value: ftd::Value::Record {
                                    name: "foo/bar#person".to_string(),
                                    fields: std::array::IntoIter::new([
                                        (
                                            s("bio"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::Value::String {
                                                    text: "Amit is CEO of FifthTry.".to_string(),
                                                    source: ftd::TextSource::Body,
                                                },
                                            },
                                        ),
                                        (
                                            s("name"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::Value::String {
                                                    text: "Amit Upadhyay".to_string(),
                                                    source: ftd::TextSource::Caption,
                                                },
                                            },
                                        ),
                                    ])
                                    .collect(),
                                },
                            },
                        ],
                        kind: ftd::p2::Kind::Record {
                            name: "foo/bar#person".to_string(),
                            default: None,
                        },
                    },
                },
                conditions: vec![],
            }),
        );

        bag.insert(
            "foo/bar#person".to_string(),
            ftd::p2::Thing::Record(ftd::p2::Record {
                name: "foo/bar#person".to_string(),
                fields: std::array::IntoIter::new([
                    (s("bio"), ftd::p2::Kind::body()),
                    (s("name"), ftd::p2::Kind::caption()),
                ])
                .collect(),
                instances: Default::default(),
                order: vec![s("name"), s("bio")],
            }),
        );

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- ftd.row foo:
                caption name:
                string body:

                --- ftd.text: $name

                --- ftd.text: $body

                -- record person:
                caption name:
                body bio:

                -- person list people:

                -- people: Arpita Jaiswal

                Arpita is developer at Fifthtry

                -- people: Amit Upadhyay

                Amit is CEO of FifthTry.

                -- ftd.column:

                --- foo: $obj.name
                $loop$: $people as $obj
                body: $obj.bio
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");
        // pretty_assertions::assert_eq!(g_bag, bag);
        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn basic_processor() {
        let mut main = super::default_column();

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("\"0.1.18\""),
                line: true,
                common: ftd::Common {
                    reference: Some(s("foo/bar#test")),
                    ..Default::default()
                },
                ..Default::default()
            }));

        let mut bag = super::default_bag();

        bag.insert(
            "foo/bar#test".to_string(),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: "foo/bar#test".to_string(),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::String {
                        text: "\"0.1.18\"".to_string(),
                        source: ftd::TextSource::Header,
                    },
                },
                conditions: vec![],
            }),
        );

        let (g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- string test:
                $processor$: read_version_from_cargo_toml

                -- ftd.text: $test
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");
        pretty_assertions::assert_eq!(g_bag, bag);
        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn basic_processor_that_overwrites() {
        let mut main = super::default_column();

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("\"0.1.18\""),
                line: true,
                common: ftd::Common {
                    reference: Some(s("foo/bar#test")),
                    ..Default::default()
                },
                ..Default::default()
            }));

        let mut bag = super::default_bag();

        bag.insert(
            "foo/bar#test".to_string(),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: "test".to_string(),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::String {
                        text: "\"0.1.18\"".to_string(),
                        source: ftd::TextSource::Header,
                    },
                },
                conditions: vec![],
            }),
        );

        let (g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- string test: yo

                -- test:
                $processor$: read_version_from_cargo_toml

                -- ftd.text: $test
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");
        pretty_assertions::assert_eq!(g_bag, bag);
        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn basic_processor_for_list() {
        let mut main = super::default_column();

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("\"ftd\""),
                line: true,
                common: ftd::Common {
                    reference: Some(s("foo/bar#test")),
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("\"0.1.18\""),
                line: true,
                common: ftd::Common {
                    reference: Some(s("foo/bar#test")),
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("["),
                line: true,
                common: ftd::Common {
                    reference: Some(s("foo/bar#test")),
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("\"2018\""),
                line: true,
                common: ftd::Common {
                    reference: Some(s("foo/bar#test")),
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("\"ftd: FifthTry Document Format\""),
                line: true,
                common: ftd::Common {
                    reference: Some(s("foo/bar#test")),
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("\"MIT\""),
                line: true,
                common: ftd::Common {
                    reference: Some(s("foo/bar#test")),
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("\"https://github.com/FifthTry/ftd\""),
                line: true,
                common: ftd::Common {
                    reference: Some(s("foo/bar#test")),
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("\"https://ftd.dev\""),
                line: true,
                common: ftd::Common {
                    reference: Some(s("foo/bar#test")),
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("$loop$"),
                line: true,
                common: ftd::Common {
                    reference: Some(s("foo/bar#test")),
                    is_dummy: true,
                    ..Default::default()
                },
                ..Default::default()
            }));

        let mut bag = super::default_bag();

        bag.insert(
            "foo/bar#test".to_string(),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: "foo/bar#test".to_string(),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::List {
                        data: vec![
                            ftd::PropertyValue::Value {
                                value: ftd::Value::String {
                                    text: "\"ftd\"".to_string(),
                                    source: ftd::TextSource::Header,
                                },
                            },
                            ftd::PropertyValue::Value {
                                value: ftd::Value::String {
                                    text: "\"0.1.18\"".to_string(),
                                    source: ftd::TextSource::Header,
                                },
                            },
                            ftd::PropertyValue::Value {
                                value: ftd::Value::String {
                                    text: "[".to_string(),
                                    source: ftd::TextSource::Header,
                                },
                            },
                            ftd::PropertyValue::Value {
                                value: ftd::Value::String {
                                    text: "\"2018\"".to_string(),
                                    source: ftd::TextSource::Header,
                                },
                            },
                            ftd::PropertyValue::Value {
                                value: ftd::Value::String {
                                    text: "\"ftd: FifthTry Document Format\"".to_string(),
                                    source: ftd::TextSource::Header,
                                },
                            },
                            ftd::PropertyValue::Value {
                                value: ftd::Value::String {
                                    text: "\"MIT\"".to_string(),
                                    source: ftd::TextSource::Header,
                                },
                            },
                            ftd::PropertyValue::Value {
                                value: ftd::Value::String {
                                    text: "\"https://github.com/FifthTry/ftd\"".to_string(),
                                    source: ftd::TextSource::Header,
                                },
                            },
                            ftd::PropertyValue::Value {
                                value: ftd::Value::String {
                                    text: "\"https://ftd.dev\"".to_string(),
                                    source: ftd::TextSource::Header,
                                },
                            },
                        ],
                        kind: ftd::p2::Kind::string(),
                    },
                },
                conditions: vec![],
            }),
        );

        let (g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- string list test:
                $processor$: read_package_from_cargo_toml

                -- ftd.text: $obj
                $loop$: $test as $obj
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        pretty_assertions::assert_eq!(g_bag, bag);
        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn processor_for_list_of_record() {
        let mut main = super::default_column();

        main.container.children.push(ftd::Element::Row(ftd::Row {
            spacing: None,
            container: ftd::Container {
                children: vec![
                    ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("\"ftd\""),
                        line: true,
                        common: ftd::Common {
                            reference: Some(s("@name@0")),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                    ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("name"),
                        line: true,
                        common: ftd::Common {
                            reference: Some(s("@body@0")),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                ],
                ..Default::default()
            },
            common: ftd::Common {
                locals: std::array::IntoIter::new([
                    (s("name@0"), s("\"ftd\"")),
                    (s("body@0"), s("name")),
                ])
                .collect(),
                reference: Some(s("foo/bar#test")),
                ..Default::default()
            },
        }));

        main.container.children.push(ftd::Element::Row(ftd::Row {
            spacing: None,
            container: ftd::Container {
                children: vec![
                    ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("\"0.1.18\""),
                        line: true,
                        common: ftd::Common {
                            reference: Some(s("@name@1")),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                    ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("version"),
                        line: true,
                        common: ftd::Common {
                            reference: Some(s("@body@1")),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                ],
                ..Default::default()
            },
            common: ftd::Common {
                locals: std::array::IntoIter::new([
                    (s("name@1"), s("\"0.1.18\"")),
                    (s("body@1"), s("version")),
                ])
                .collect(),
                reference: Some(s("foo/bar#test")),
                ..Default::default()
            },
        }));

        main.container.children.push(ftd::Element::Row(ftd::Row {
            spacing: None,
            container: ftd::Container {
                children: vec![
                    ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("["),
                        line: true,
                        common: ftd::Common {
                            reference: Some(s("@name@2")),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                    ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("authors"),
                        line: true,
                        common: ftd::Common {
                            reference: Some(s("@body@2")),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                ],
                ..Default::default()
            },
            common: ftd::Common {
                locals: std::array::IntoIter::new([
                    (s("name@2"), s("[")),
                    (s("body@2"), s("authors")),
                ])
                .collect(),
                reference: Some(s("foo/bar#test")),
                ..Default::default()
            },
        }));

        main.container.children.push(ftd::Element::Row(ftd::Row {
            spacing: None,
            container: ftd::Container {
                children: vec![
                    ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("\"2018\""),
                        line: true,
                        common: ftd::Common {
                            reference: Some(s("@name@3")),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                    ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("edition"),
                        line: true,
                        common: ftd::Common {
                            reference: Some(s("@body@3")),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                ],
                ..Default::default()
            },
            common: ftd::Common {
                locals: std::array::IntoIter::new([
                    (s("name@3"), s("\"2018\"")),
                    (s("body@3"), s("edition")),
                ])
                .collect(),
                reference: Some(s("foo/bar#test")),
                ..Default::default()
            },
        }));

        main.container.children.push(ftd::Element::Row(ftd::Row {
            spacing: None,
            container: ftd::Container {
                children: vec![
                    ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("\"ftd: FifthTry Document Format\""),
                        line: true,
                        common: ftd::Common {
                            reference: Some(s("@name@4")),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                    ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("description"),
                        line: true,
                        common: ftd::Common {
                            reference: Some(s("@body@4")),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                ],
                ..Default::default()
            },
            common: ftd::Common {
                locals: std::array::IntoIter::new([
                    (s("name@4"), s("\"ftd: FifthTry Document Format\"")),
                    (s("body@4"), s("description")),
                ])
                .collect(),
                reference: Some(s("foo/bar#test")),
                ..Default::default()
            },
        }));

        main.container.children.push(ftd::Element::Row(ftd::Row {
            spacing: None,
            container: ftd::Container {
                children: vec![
                    ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("\"MIT\""),
                        line: true,
                        common: ftd::Common {
                            reference: Some(s("@name@5")),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                    ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("license"),
                        line: true,
                        common: ftd::Common {
                            reference: Some(s("@body@5")),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                ],
                ..Default::default()
            },
            common: ftd::Common {
                locals: std::array::IntoIter::new([
                    (s("name@5"), s("\"MIT\"")),
                    (s("body@5"), s("license")),
                ])
                .collect(),
                reference: Some(s("foo/bar#test")),
                ..Default::default()
            },
        }));

        main.container.children.push(ftd::Element::Row(ftd::Row {
            spacing: None,
            container: ftd::Container {
                children: vec![
                    ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("\"https://github.com/FifthTry/ftd\""),
                        line: true,
                        common: ftd::Common {
                            reference: Some(s("@name@6")),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                    ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("repository"),
                        line: true,
                        common: ftd::Common {
                            reference: Some(s("@body@6")),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                ],
                ..Default::default()
            },
            common: ftd::Common {
                locals: std::array::IntoIter::new([
                    (s("name@6"), s("\"https://github.com/FifthTry/ftd\"")),
                    (s("body@6"), s("repository")),
                ])
                .collect(),
                reference: Some(s("foo/bar#test")),
                ..Default::default()
            },
        }));

        main.container.children.push(ftd::Element::Row(ftd::Row {
            spacing: None,
            container: ftd::Container {
                children: vec![
                    ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("\"https://ftd.dev\""),
                        line: true,
                        common: ftd::Common {
                            reference: Some(s("@name@7")),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                    ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("homepage"),
                        line: true,
                        common: ftd::Common {
                            reference: Some(s("@body@7")),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                ],
                ..Default::default()
            },
            common: ftd::Common {
                locals: std::array::IntoIter::new([
                    (s("name@7"), s("\"https://ftd.dev\"")),
                    (s("body@7"), s("homepage")),
                ])
                .collect(),
                reference: Some(s("foo/bar#test")),
                ..Default::default()
            },
        }));

        let mut bag = super::default_bag();

        bag.insert(
            "foo/bar#data".to_string(),
            ftd::p2::Thing::Record(ftd::p2::Record {
                name: "foo/bar#data".to_string(),
                fields: std::array::IntoIter::new([
                    (s("description"), ftd::p2::Kind::string()),
                    (s("title"), ftd::p2::Kind::string()),
                ])
                .collect(),
                instances: Default::default(),
                order: vec![s("title"), s("description")],
            }),
        );

        bag.insert(
            "foo/bar#foo".to_string(),
            ftd::p2::Thing::Component(ftd::Component {
                root: "ftd.row".to_string(),
                full_name: "foo/bar#foo".to_string(),
                arguments: std::array::IntoIter::new([
                    (s("body"), ftd::p2::Kind::string()),
                    (s("name"), ftd::p2::Kind::caption()),
                ])
                .collect(),
                instructions: vec![
                    ftd::component::Instruction::ChildComponent {
                        child: ftd::component::ChildComponent {
                            is_recursive: false,
                            events: vec![],
                            root: "ftd#text".to_string(),
                            condition: None,
                            properties: std::array::IntoIter::new([(
                                s("text"),
                                ftd::component::Property {
                                    default: Some(ftd::PropertyValue::Variable {
                                        name: "name".to_string(),
                                        kind: ftd::p2::Kind::caption_or_body(),
                                    }),
                                    conditions: vec![],
                                    ..Default::default()
                                },
                            )])
                            .collect(),
                            ..Default::default()
                        },
                    },
                    ftd::component::Instruction::ChildComponent {
                        child: ftd::component::ChildComponent {
                            is_recursive: false,
                            events: vec![],
                            root: "ftd#text".to_string(),
                            condition: None,
                            properties: std::array::IntoIter::new([(
                                s("text"),
                                ftd::component::Property {
                                    default: Some(ftd::PropertyValue::Variable {
                                        name: "body".to_string(),
                                        kind: ftd::p2::Kind::caption_or_body(),
                                    }),
                                    conditions: vec![],
                                    ..Default::default()
                                },
                            )])
                            .collect(),
                            ..Default::default()
                        },
                    },
                ],
                ..Default::default()
            }),
        );

        bag.insert(
            "foo/bar#test".to_string(),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: "foo/bar#test".to_string(),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::List {
                        data: vec![
                            ftd::PropertyValue::Value {
                                value: ftd::Value::Record {
                                    name: "foo/bar#data".to_string(),
                                    fields: std::array::IntoIter::new([
                                        (
                                            s("description"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::variable::Value::String {
                                                    text: "name".to_string(),
                                                    source: ftd::TextSource::Header,
                                                },
                                            },
                                        ),
                                        (
                                            s("title"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::variable::Value::String {
                                                    text: "\"ftd\"".to_string(),
                                                    source: ftd::TextSource::Header,
                                                },
                                            },
                                        ),
                                    ])
                                    .collect(),
                                },
                            },
                            ftd::PropertyValue::Value {
                                value: ftd::Value::Record {
                                    name: "foo/bar#data".to_string(),
                                    fields: std::array::IntoIter::new([
                                        (
                                            s("description"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::variable::Value::String {
                                                    text: "version".to_string(),
                                                    source: ftd::TextSource::Header,
                                                },
                                            },
                                        ),
                                        (
                                            s("title"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::variable::Value::String {
                                                    text: "\"0.1.18\"".to_string(),
                                                    source: ftd::TextSource::Header,
                                                },
                                            },
                                        ),
                                    ])
                                    .collect(),
                                },
                            },
                            ftd::PropertyValue::Value {
                                value: ftd::Value::Record {
                                    name: "foo/bar#data".to_string(),
                                    fields: std::array::IntoIter::new([
                                        (
                                            s("description"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::variable::Value::String {
                                                    text: "authors".to_string(),
                                                    source: ftd::TextSource::Header,
                                                },
                                            },
                                        ),
                                        (
                                            s("title"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::variable::Value::String {
                                                    text: "[".to_string(),
                                                    source: ftd::TextSource::Header,
                                                },
                                            },
                                        ),
                                    ])
                                    .collect(),
                                },
                            },
                            ftd::PropertyValue::Value {
                                value: ftd::Value::Record {
                                    name: "foo/bar#data".to_string(),
                                    fields: std::array::IntoIter::new([
                                        (
                                            s("description"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::variable::Value::String {
                                                    text: "edition".to_string(),
                                                    source: ftd::TextSource::Header,
                                                },
                                            },
                                        ),
                                        (
                                            s("title"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::variable::Value::String {
                                                    text: "\"2018\"".to_string(),
                                                    source: ftd::TextSource::Header,
                                                },
                                            },
                                        ),
                                    ])
                                    .collect(),
                                },
                            },
                            ftd::PropertyValue::Value {
                                value: ftd::Value::Record {
                                    name: "foo/bar#data".to_string(),
                                    fields: std::array::IntoIter::new([
                                        (
                                            s("description"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::variable::Value::String {
                                                    text: "description".to_string(),
                                                    source: ftd::TextSource::Header,
                                                },
                                            },
                                        ),
                                        (
                                            s("title"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::variable::Value::String {
                                                    text: "\"ftd: FifthTry Document Format\""
                                                        .to_string(),
                                                    source: ftd::TextSource::Header,
                                                },
                                            },
                                        ),
                                    ])
                                    .collect(),
                                },
                            },
                            ftd::PropertyValue::Value {
                                value: ftd::Value::Record {
                                    name: "foo/bar#data".to_string(),
                                    fields: std::array::IntoIter::new([
                                        (
                                            s("description"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::variable::Value::String {
                                                    text: "license".to_string(),
                                                    source: ftd::TextSource::Header,
                                                },
                                            },
                                        ),
                                        (
                                            s("title"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::variable::Value::String {
                                                    text: "\"MIT\"".to_string(),
                                                    source: ftd::TextSource::Header,
                                                },
                                            },
                                        ),
                                    ])
                                    .collect(),
                                },
                            },
                            ftd::PropertyValue::Value {
                                value: ftd::Value::Record {
                                    name: "foo/bar#data".to_string(),
                                    fields: std::array::IntoIter::new([
                                        (
                                            s("description"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::variable::Value::String {
                                                    text: "repository".to_string(),
                                                    source: ftd::TextSource::Header,
                                                },
                                            },
                                        ),
                                        (
                                            s("title"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::variable::Value::String {
                                                    text: "\"https://github.com/FifthTry/ftd\""
                                                        .to_string(),
                                                    source: ftd::TextSource::Header,
                                                },
                                            },
                                        ),
                                    ])
                                    .collect(),
                                },
                            },
                            ftd::PropertyValue::Value {
                                value: ftd::Value::Record {
                                    name: "foo/bar#data".to_string(),
                                    fields: std::array::IntoIter::new([
                                        (
                                            s("description"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::variable::Value::String {
                                                    text: "homepage".to_string(),
                                                    source: ftd::TextSource::Header,
                                                },
                                            },
                                        ),
                                        (
                                            s("title"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::variable::Value::String {
                                                    text: "\"https://ftd.dev\"".to_string(),
                                                    source: ftd::TextSource::Header,
                                                },
                                            },
                                        ),
                                    ])
                                    .collect(),
                                },
                            },
                        ],
                        kind: ftd::p2::Kind::Record {
                            name: s("foo/bar#data"),
                            default: None,
                        },
                    },
                },
                conditions: vec![],
            }),
        );

        p!(
            "
            -- ftd.row foo:
            caption name:
            string body:

            --- ftd.text: $name

            --- ftd.text: $body

            -- record data:
            string title:
            string description:

            -- data list test:
            $processor$: read_package_records_from_cargo_toml

            -- foo: $obj.title
            $loop$: $test as $obj
            body: $obj.description
            ",
            (bag, main),
        );
    }

    #[test]
    fn loop_with_tree_structure() {
        let mut main = super::default_column();
        let col = ftd::Element::Column(ftd::Column {
            spacing: None,
            container: ftd::Container {
                children: vec![
                    ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("ab title"),
                        line: true,
                        common: ftd::Common {
                            reference: Some(s("@toc.title")),
                            link: Some(s("ab link")),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                    ftd::Element::Column(ftd::Column {
                        spacing: None,
                        container: ftd::Container {
                            children: vec![ftd::Element::Markup(ftd::Markups {
                                text: ftd::markdown_line("aa title"),
                                line: true,
                                common: ftd::Common {
                                    reference: Some(s("@toc.title")),
                                    link: Some(s("aa link")),
                                    ..Default::default()
                                },
                                ..Default::default()
                            })],
                            ..Default::default()
                        },
                        common: ftd::Common {
                            locals: std::array::IntoIter::new([(
                                s("toc@0,1"),
                                s("{\"link\":\"aa link\",\"title\":\"aa title\"}"),
                            )])
                            .collect(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                    ftd::Element::Column(ftd::Column {
                        spacing: None,
                        container: ftd::Container {
                            children: vec![ftd::Element::Markup(ftd::Markups {
                                text: ftd::markdown_line("aaa title"),
                                line: true,
                                common: ftd::Common {
                                    reference: Some(s("@toc.title")),
                                    link: Some(s("aaa link")),
                                    ..Default::default()
                                },
                                ..Default::default()
                            })],
                            ..Default::default()
                        },
                        common: ftd::Common {
                            locals: std::array::IntoIter::new([(
                                s("toc@0,2"),
                                s("{\"link\":\"aaa link\",\"title\":\"aaa title\"}"),
                            )])
                            .collect(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                ],
                ..Default::default()
            },
            common: ftd::Common {
                reference: Some(s("foo/bar#toc")),
                locals: std::array::IntoIter::new([(
                    s("toc@0"),
                    s("{\"link\":\"ab link\",\"title\":\"ab title\"}"),
                )])
                .collect(),
                ..Default::default()
            },
            ..Default::default()
        });
        let col1 = ftd::Element::Column(ftd::Column {
            spacing: None,
            container: ftd::Container {
                children: vec![
                    ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("ab title"),
                        line: true,
                        common: ftd::Common {
                            reference: Some(s("@toc.title")),
                            link: Some(s("ab link")),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                    ftd::Element::Column(ftd::Column {
                        spacing: None,
                        container: ftd::Container {
                            children: vec![ftd::Element::Markup(ftd::Markups {
                                text: ftd::markdown_line("aa title"),
                                line: true,
                                common: ftd::Common {
                                    reference: Some(s("@toc.title")),
                                    link: Some(s("aa link")),
                                    ..Default::default()
                                },
                                ..Default::default()
                            })],
                            ..Default::default()
                        },
                        common: ftd::Common {
                            locals: std::array::IntoIter::new([(
                                s("toc@1,0,1"),
                                s("{\"link\":\"aa link\",\"title\":\"aa title\"}"),
                            )])
                            .collect(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                    ftd::Element::Column(ftd::Column {
                        spacing: None,
                        container: ftd::Container {
                            children: vec![ftd::Element::Markup(ftd::Markups {
                                text: ftd::markdown_line("aaa title"),
                                line: true,
                                common: ftd::Common {
                                    reference: Some(s("@toc.title")),
                                    link: Some(s("aaa link")),
                                    ..Default::default()
                                },
                                ..Default::default()
                            })],
                            ..Default::default()
                        },
                        common: ftd::Common {
                            locals: std::array::IntoIter::new([(
                                s("toc@1,0,2"),
                                s("{\"link\":\"aaa link\",\"title\":\"aaa title\"}"),
                            )])
                            .collect(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                ],
                ..Default::default()
            },
            common: ftd::Common {
                reference: Some(s("foo/bar#toc")),
                locals: std::array::IntoIter::new([(
                    s("toc@1,0"),
                    s("{\"link\":\"ab link\",\"title\":\"ab title\"}"),
                )])
                .collect(),
                ..Default::default()
            },
            ..Default::default()
        });
        main.container.children.push(col.clone());
        main.container.children.push(ftd::Element::Row(ftd::Row {
            spacing: None,
            container: ftd::Container {
                children: vec![col1],
                ..Default::default()
            },
            ..Default::default()
        }));

        let mut bag = super::default_bag();

        bag.insert(
            s("foo/bar#aa"),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: s("foo/bar#aa"),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::List {
                        data: vec![
                            ftd::PropertyValue::Value {
                                value: ftd::Value::Record {
                                    name: s("foo/bar#toc-record"),
                                    fields: std::array::IntoIter::new([
                                        (
                                            s("children"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::variable::Value::List {
                                                    data: vec![],
                                                    kind: ftd::p2::Kind::Record {
                                                        name: s("foo/bar#toc-record"),
                                                        default: None,
                                                    },
                                                },
                                            },
                                        ),
                                        (
                                            s("link"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::variable::Value::String {
                                                    text: s("aa link"),
                                                    source: ftd::TextSource::Header,
                                                },
                                            },
                                        ),
                                        (
                                            s("title"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::variable::Value::String {
                                                    text: s("aa title"),
                                                    source: ftd::TextSource::Header,
                                                },
                                            },
                                        ),
                                    ])
                                    .collect(),
                                },
                            },
                            ftd::PropertyValue::Value {
                                value: ftd::Value::Record {
                                    name: s("foo/bar#toc-record"),
                                    fields: std::array::IntoIter::new([
                                        (
                                            s("children"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::variable::Value::List {
                                                    data: vec![],
                                                    kind: ftd::p2::Kind::Record {
                                                        name: s("foo/bar#toc-record"),
                                                        default: None,
                                                    },
                                                },
                                            },
                                        ),
                                        (
                                            s("link"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::variable::Value::String {
                                                    text: s("aaa link"),
                                                    source: ftd::TextSource::Header,
                                                },
                                            },
                                        ),
                                        (
                                            s("title"),
                                            ftd::PropertyValue::Value {
                                                value: ftd::variable::Value::String {
                                                    text: s("aaa title"),
                                                    source: ftd::TextSource::Header,
                                                },
                                            },
                                        ),
                                    ])
                                    .collect(),
                                },
                            },
                        ],
                        kind: ftd::p2::Kind::Record {
                            name: s("foo/bar#toc-record"),
                            default: None,
                        },
                    },
                },
                conditions: vec![],
            }),
        );

        bag.insert(
            s("foo/bar#toc"),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: s("foo/bar#toc"),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::List {
                        data: vec![ftd::PropertyValue::Value {
                            value: ftd::Value::Record {
                                name: s("foo/bar#toc-record"),
                                fields: std::array::IntoIter::new([
                                    (
                                        s("children"),
                                        ftd::PropertyValue::Value {
                                            value: ftd::variable::Value::List {
                                                data: vec![
                                                    ftd::PropertyValue::Value {value: ftd::Value::Record {
                                                        name: s("foo/bar#toc-record"),
                                                        fields: std::array::IntoIter::new([
                                                            (
                                                                s("children"),
                                                                ftd::PropertyValue::Value {
                                                                    value: ftd::variable::Value::List {
                                                                        data: vec![],
                                                                        kind: ftd::p2::Kind::Record {
                                                                            name: s("foo/bar#toc-record"),
                                                                            default: None,
                                                                        },
                                                                    },
                                                                },
                                                            ),
                                                            (
                                                                s("link"),
                                                                ftd::PropertyValue::Value {
                                                                    value: ftd::variable::Value::String {
                                                                        text: s("aa link"),
                                                                        source: ftd::TextSource::Header,
                                                                    },
                                                                },
                                                            ),
                                                            (
                                                                s("title"),
                                                                ftd::PropertyValue::Value {
                                                                    value: ftd::variable::Value::String {
                                                                        text: s("aa title"),
                                                                        source: ftd::TextSource::Header,
                                                                    },
                                                                },
                                                            ),
                                                        ])
                                                        .collect(),
                                                    }},
                                                    ftd::PropertyValue::Value {value: ftd::Value::Record {
                                                        name: s("foo/bar#toc-record"),
                                                        fields: std::array::IntoIter::new([
                                                            (
                                                                s("children"),
                                                                ftd::PropertyValue::Value {
                                                                    value: ftd::variable::Value::List {
                                                                        data: vec![],
                                                                        kind: ftd::p2::Kind::Record {
                                                                            name: s("foo/bar#toc-record"),
                                                                            default: None,
                                                                        },
                                                                    },
                                                                },
                                                            ),
                                                            (
                                                                s("link"),
                                                                ftd::PropertyValue::Value {
                                                                    value: ftd::variable::Value::String {
                                                                        text: s("aaa link"),
                                                                        source: ftd::TextSource::Header,
                                                                    },
                                                                },
                                                            ),
                                                            (
                                                                s("title"),
                                                                ftd::PropertyValue::Value {
                                                                    value: ftd::variable::Value::String {
                                                                        text: s("aaa title"),
                                                                        source: ftd::TextSource::Header,
                                                                    },
                                                                },
                                                            ),
                                                        ])
                                                        .collect(),
                                                    }},
                                                ],
                                                kind: ftd::p2::Kind::Record {
                                                    name: s("foo/bar#toc-record"),
                                                    default: None,
                                                },
                                            },
                                        },
                                    ),
                                    (
                                        s("link"),
                                        ftd::PropertyValue::Value {
                                            value: ftd::variable::Value::String {
                                                text: s("ab link"),
                                                source: ftd::TextSource::Header,
                                            },
                                        },
                                    ),
                                    (
                                        s("title"),
                                        ftd::PropertyValue::Value {
                                            value: ftd::variable::Value::String {
                                                text: s("ab title"),
                                                source: ftd::TextSource::Header,
                                            },
                                        },
                                    ),
                                ])
                                .collect(),
                            },
                        }],
                        kind: ftd::p2::Kind::Record {
                            name: s("foo/bar#toc-record"),
                            default: None,
                        },
                    },
                },
                conditions: vec![],
            }),
        );

        bag.insert(
            s("foo/bar#toc"),
            ftd::p2::Thing::Component(ftd::Component {
                root: "ftd.column".to_string(),
                full_name: "foo/bar#toc-item".to_string(),
                arguments: std::array::IntoIter::new([(
                    s("toc"),
                    ftd::p2::Kind::Record {
                        name: "foo/bar#toc-record".to_string(),
                        default: None,
                    },
                )])
                .collect(),
                instructions: vec![
                    Instruction::ChildComponent {
                        child: ftd::ChildComponent {
                            events: vec![],
                            root: "ftd#text".to_string(),
                            condition: None,
                            properties: std::array::IntoIter::new([
                                (
                                    s("link"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Variable {
                                            name: "toc.link".to_string(),
                                            kind: ftd::p2::Kind::Optional {
                                                kind: Box::new(ftd::p2::Kind::string()),
                                            },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                                (
                                    s("text"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Variable {
                                            name: "toc.title".to_string(),
                                            kind: ftd::p2::Kind::Optional {
                                                kind: Box::new(ftd::p2::Kind::caption_or_body()),
                                            },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                            ])
                            .collect(),
                            ..Default::default()
                        },
                    },
                    Instruction::RecursiveChildComponent {
                        child: ftd::ChildComponent {
                            is_recursive: true,
                            events: vec![],
                            root: "toc-item".to_string(),
                            condition: None,
                            properties: std::array::IntoIter::new([
                                (
                                    s("$loop$"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Variable {
                                            name: "toc.children".to_string(),
                                            kind: ftd::p2::Kind::Record {
                                                name: s("foo/bar#toc-record"),
                                                default: None,
                                            },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                                (
                                    s("toc"),
                                    ftd::component::Property {
                                        default: Some(ftd::PropertyValue::Variable {
                                            name: "$loop$".to_string(),
                                            kind: ftd::p2::Kind::Record {
                                                name: s("foo/bar#toc-record"),
                                                default: None,
                                            },
                                        }),
                                        conditions: vec![],
                                        ..Default::default()
                                    },
                                ),
                            ])
                            .collect(),
                            ..Default::default()
                        },
                    },
                ],
                ..Default::default()
            }),
        );

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- record toc-record:
                string title:
                string link:
                toc-record list children:

                -- ftd.column toc-item:
                toc-record toc:

                --- ftd.text: $toc.title
                link: $toc.link

                --- toc-item:
                $loop$: $toc.children as $obj
                toc: $obj

                -- toc-record list aa:

                -- aa:
                title: aa title
                link: aa link

                -- aa:
                title: aaa title
                link: aaa link

                -- toc-record list toc:

                -- toc:
                title: ab title
                link: ab link
                children: $aa

                -- ftd.row foo:

                --- toc-item:
                $loop$: $toc as $obj
                toc: $obj

                -- toc-item:
                $loop$: $toc as $obj
                toc: $obj

                -- foo:
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        // pretty_assertions::assert_eq!(g_bag, bag);
        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn import_check() {
        let mut main = super::default_column();
        main.container.children.push(ftd::Element::Row(ftd::Row {
            spacing: None,
            container: ftd::Container {
                children: vec![ftd::Element::Markup(ftd::Markups {
                    text: ftd::markdown_line("Hello World"),
                    line: true,
                    common: ftd::Common {
                        reference: Some(s("hello-world-variable#hello-world")),
                        ..Default::default()
                    },
                    ..Default::default()
                })],
                ..Default::default()
            },
            ..Default::default()
        }));

        let mut bag = super::default_bag();
        bag.insert(
            s("hello-world#foo"),
            ftd::p2::Thing::Component(ftd::Component {
                root: s("ftd.row"),
                full_name: s("hello-world#foo"),
                instructions: vec![ftd::Instruction::ChildComponent {
                    child: ftd::ChildComponent {
                        events: vec![],
                        root: s("ftd#text"),
                        condition: None,
                        properties: std::array::IntoIter::new([(
                            s("text"),
                            ftd::component::Property {
                                default: Some(ftd::PropertyValue::Reference {
                                    name: "hello-world-variable#hello-world".to_string(),
                                    kind: ftd::p2::Kind::caption_or_body(),
                                }),
                                conditions: vec![],
                                ..Default::default()
                            },
                        )])
                        .collect(),
                        ..Default::default()
                    },
                }],
                invocations: vec![],
                ..Default::default()
            }),
        );
        bag.insert(
            s("hello-world-variable#hello-world"),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: s("hello-world"),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::String {
                        text: s("Hello World"),
                        source: ftd::TextSource::Caption,
                    },
                },
                conditions: vec![],
            }),
        );

        p!(
            "
            -- import: hello-world as hw

            -- hw.foo:
            ",
            (bag, main),
        );
    }

    #[test]
    fn argument_with_default_value() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("hello world"),
                line: true,
                size: Some(10),
                common: ftd::Common {
                    locals: std::array::IntoIter::new([
                        (s("name@0"), s("hello world")),
                        (s("size@0"), s("10")),
                    ])
                    .collect(),
                    reference: Some(s("@name@0")),
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("hello"),
                line: true,
                size: Some(10),
                common: ftd::Common {
                    locals: std::array::IntoIter::new([
                        (s("name@1"), s("hello")),
                        (s("size@1"), s("10")),
                    ])
                    .collect(),
                    reference: Some(s("@name@1")),
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("this is nice"),
                line: true,
                size: Some(20),
                common: ftd::Common {
                    locals: std::array::IntoIter::new([
                        (s("name@2"), s("this is nice")),
                        (s("size@2"), s("20")),
                    ])
                    .collect(),
                    reference: Some(s("@name@2")),
                    ..Default::default()
                },
                ..Default::default()
            }));

        let mut bag = super::default_bag();
        bag.insert(
            s("foo/bar#foo"),
            ftd::p2::Thing::Component(ftd::Component {
                root: s("ftd.text"),
                full_name: s("foo/bar#foo"),
                arguments: std::array::IntoIter::new([
                    (
                        s("name"),
                        ftd::p2::Kind::caption().set_default(Some(s("hello world"))),
                    ),
                    (
                        s("size"),
                        ftd::p2::Kind::Integer {
                            default: Some(s("10")),
                        },
                    ),
                ])
                .collect(),
                properties: std::array::IntoIter::new([
                    (
                        s("size"),
                        ftd::component::Property {
                            default: Some(ftd::PropertyValue::Variable {
                                name: s("size"),
                                kind: ftd::p2::Kind::Optional {
                                    kind: Box::from(ftd::p2::Kind::Integer {
                                        default: Some(s("10")),
                                    }),
                                },
                            }),
                            conditions: vec![],
                            ..Default::default()
                        },
                    ),
                    (
                        s("text"),
                        ftd::component::Property {
                            default: Some(ftd::PropertyValue::Variable {
                                name: s("name"),
                                kind: ftd::p2::Kind::caption_or_body()
                                    .set_default(Some(s("hello world"))),
                            }),
                            conditions: vec![],
                            ..Default::default()
                        },
                    ),
                ])
                .collect(),
                invocations: vec![
                    std::array::IntoIter::new([
                        (
                            s("name"),
                            ftd::Value::String {
                                text: s("hello world"),
                                source: ftd::TextSource::Default,
                            },
                        ),
                        (s("size"), ftd::Value::Integer { value: 10 }),
                    ])
                    .collect(),
                    std::array::IntoIter::new([
                        (
                            s("name"),
                            ftd::Value::String {
                                text: s("hello"),
                                source: ftd::TextSource::Caption,
                            },
                        ),
                        (s("size"), ftd::Value::Integer { value: 10 }),
                    ])
                    .collect(),
                    std::array::IntoIter::new([
                        (
                            s("name"),
                            ftd::Value::String {
                                text: s("this is nice"),
                                source: ftd::TextSource::Caption,
                            },
                        ),
                        (s("size"), ftd::Value::Integer { value: 20 }),
                    ])
                    .collect(),
                ],
                line_number: 1,
                ..Default::default()
            }),
        );

        let (g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- ftd.text foo:
                caption name: hello world
                integer size: 10
                text: $name
                size: $size

                -- foo:

                -- foo: hello

                -- foo: this is nice
                size: 20
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        pretty_assertions::assert_eq!(g_bag, bag);
        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn record_with_default_value() {
        let mut bag = super::default_bag();
        bag.insert(
            s("foo/bar#abrar"),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: s("abrar"),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::Record {
                        name: s("foo/bar#person"),
                        fields: std::array::IntoIter::new([
                            (
                                s("address"),
                                ftd::PropertyValue::Value {
                                    value: ftd::variable::Value::String {
                                        text: s("Bihar"),
                                        source: ftd::TextSource::Default,
                                    },
                                },
                            ),
                            (
                                s("age"),
                                ftd::PropertyValue::Reference {
                                    name: s("foo/bar#default-age"),
                                    kind: ftd::p2::Kind::Integer {
                                        default: Some(s("$foo/bar#default-age")),
                                    },
                                },
                            ),
                            (
                                s("bio"),
                                ftd::PropertyValue::Value {
                                    value: ftd::variable::Value::String {
                                        text: s("Software developer working at fifthtry."),
                                        source: ftd::TextSource::Body,
                                    },
                                },
                            ),
                            (
                                s("name"),
                                ftd::PropertyValue::Reference {
                                    name: s("foo/bar#abrar-name"),
                                    kind: ftd::p2::Kind::caption(),
                                },
                            ),
                            (
                                s("size"),
                                ftd::PropertyValue::Value {
                                    value: ftd::variable::Value::Integer { value: 10 },
                                },
                            ),
                        ])
                        .collect(),
                    },
                },
                conditions: vec![],
            }),
        );
        bag.insert(
            s("foo/bar#abrar-name"),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: s("abrar-name"),
                value: ftd::PropertyValue::Value {
                    value: ftd::variable::Value::String {
                        text: s("Abrar Khan"),
                        source: ftd::TextSource::Caption,
                    },
                },
                conditions: vec![],
            }),
        );
        bag.insert(
            s("foo/bar#default-age"),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: s("default-age"),
                value: ftd::PropertyValue::Value {
                    value: ftd::variable::Value::Integer { value: 20 },
                },
                conditions: vec![],
            }),
        );
        bag.insert(
            s("foo/bar#person"),
            ftd::p2::Thing::Record(ftd::p2::Record {
                name: s("foo/bar#person"),
                fields: std::array::IntoIter::new([
                    (
                        s("address"),
                        ftd::p2::Kind::string().set_default(Some(s("Bihar"))),
                    ),
                    (
                        s("age"),
                        ftd::p2::Kind::Integer {
                            default: Some(s("$foo/bar#default-age")),
                        },
                    ),
                    (
                        s("bio"),
                        ftd::p2::Kind::body().set_default(Some(s("Some Bio"))),
                    ),
                    (s("name"), ftd::p2::Kind::caption()),
                    (
                        s("size"),
                        ftd::p2::Kind::Integer {
                            default: Some(s("10")),
                        },
                    ),
                ])
                .collect(),
                instances: Default::default(),
                order: vec![s("name"), s("address"), s("bio"), s("age"), s("size")],
            }),
        );

        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("Software developer working at fifthtry."),
                line: true,
                size: Some(20),
                common: ftd::Common {
                    reference: Some(s("foo/bar#abrar.bio")),
                    ..Default::default()
                },
                ..Default::default()
            }));

        let (g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- integer default-age: 20

                -- record person:
                caption name:
                string address: Bihar
                body bio: Some Bio
                integer age: $default-age
                integer size: 10

                -- string abrar-name: Abrar Khan

                -- person abrar: $abrar-name

                Software developer working at fifthtry.

                -- ftd.text: $abrar.bio
                size: $abrar.age
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        pretty_assertions::assert_eq!(g_bag, bag);
        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn default_with_reference() {
        let mut main = super::default_column();
        main.container.children.push(ftd::Element::Row(ftd::Row {
            spacing: None,
            container: ftd::Container {
                children: vec![ftd::Element::Markup(ftd::Markups {
                    text: ftd::markdown_line("Arpita"),
                    line: true,
                    size: Some(10),
                    common: ftd::Common {
                        reference: Some(s("@name@0")),
                        ..Default::default()
                    },
                    ..Default::default()
                })],
                ..Default::default()
            },
            common: ftd::Common {
                locals: std::array::IntoIter::new([
                    (s("name@0"), s("Arpita")),
                    (s("text-size@0"), s("10")),
                ])
                .collect(),
                ..Default::default()
            },
        }));
        main.container.children.push(ftd::Element::Row(ftd::Row {
            spacing: None,
            container: ftd::Container {
                children: vec![ftd::Element::Markup(ftd::Markups {
                    text: ftd::markdown_line("Amit Upadhyay"),
                    line: true,
                    size: Some(20),
                    common: ftd::Common {
                        reference: Some(s("@name@1")),
                        ..Default::default()
                    },
                    ..Default::default()
                })],
                ..Default::default()
            },
            common: ftd::Common {
                locals: std::array::IntoIter::new([
                    (s("name@1"), s("Amit Upadhyay")),
                    (s("text-size@1"), s("20")),
                ])
                .collect(),
                ..Default::default()
            },
        }));

        let mut bag = super::default_bag();
        bag.insert(
            s("foo/bar#default-name"),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: s("default-name"),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::String {
                        text: s("Arpita"),
                        source: ftd::TextSource::Caption,
                    },
                },
                conditions: vec![],
            }),
        );
        bag.insert(
            s("foo/bar#default-size"),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: s("default-size"),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::Integer { value: 10 },
                },
                conditions: vec![],
            }),
        );
        bag.insert(
            s("foo/bar#foo"),
            ftd::p2::Thing::Component(ftd::Component {
                root: s("ftd.row"),
                full_name: s("foo/bar#foo"),
                arguments: std::array::IntoIter::new([
                    (
                        s("name"),
                        ftd::p2::Kind::string().set_default(Some(s("$foo/bar#default-name"))),
                    ),
                    (
                        s("text-size"),
                        ftd::p2::Kind::Integer {
                            default: Some(s("$foo/bar#default-size")),
                        },
                    ),
                ])
                .collect(),
                instructions: vec![ftd::Instruction::ChildComponent {
                    child: ftd::ChildComponent {
                        events: vec![],
                        root: s("ftd#text"),
                        condition: None,
                        properties: std::array::IntoIter::new([
                            (
                                s("size"),
                                ftd::component::Property {
                                    default: Some(ftd::PropertyValue::Variable {
                                        name: s("text-size"),
                                        kind: ftd::p2::Kind::Optional {
                                            kind: Box::new(ftd::p2::Kind::Integer {
                                                default: Some(s("$foo/bar#default-size")),
                                            }),
                                        },
                                    }),
                                    conditions: vec![],
                                    ..Default::default()
                                },
                            ),
                            (
                                s("text"),
                                ftd::component::Property {
                                    default: Some(ftd::PropertyValue::Variable {
                                        name: s("name"),
                                        kind: ftd::p2::Kind::caption_or_body()
                                            .set_default(Some(s("$foo/bar#default-name"))),
                                    }),
                                    conditions: vec![],
                                    ..Default::default()
                                },
                            ),
                        ])
                        .collect(),
                        ..Default::default()
                    },
                }],
                kernel: false,
                ..Default::default()
            }),
        );

        p!(
            "
            -- string default-name: Arpita

            -- integer default-size: 10

            -- ftd.row foo:
            string name: $default-name
            integer text-size: $default-size

            --- ftd.text: $name
            size: $text-size

            -- foo:

            -- foo:
            name: Amit Upadhyay
            text-size: 20
            ",
            (bag, main),
        );
    }

    #[test]
    fn or_type_with_default_value() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("Amit Upadhyay"),
                line: true,
                common: ftd::Common {
                    reference: Some(s("foo/bar#amitu.name")),
                    ..Default::default()
                },
                ..Default::default()
            }));
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("1000"),
                line: true,
                common: ftd::Common {
                    reference: Some(s("foo/bar#amitu.phone")),
                    ..Default::default()
                },
                ..Default::default()
            }));
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("John Doe"),
                line: true,
                size: Some(50),
                common: ftd::Common {
                    reference: Some(s("foo/bar#acme.contact")),
                    ..Default::default()
                },
                ..Default::default()
            }));

        let mut bag = super::default_bag();
        bag.insert(
            s("foo/bar#acme"),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: s("acme"),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::OrType {
                        name: s("foo/bar#lead"),
                        variant: s("company"),
                        fields: std::array::IntoIter::new([
                            (
                                s("contact"),
                                ftd::PropertyValue::Value {
                                    value: ftd::Value::String {
                                        text: s("John Doe"),
                                        source: ftd::TextSource::Header,
                                    },
                                },
                            ),
                            (
                                s("fax"),
                                ftd::PropertyValue::Value {
                                    value: ftd::Value::String {
                                        text: s("+1-234-567890"),
                                        source: ftd::TextSource::Header,
                                    },
                                },
                            ),
                            (
                                s("name"),
                                ftd::PropertyValue::Value {
                                    value: ftd::Value::String {
                                        text: s("Acme Inc."),
                                        source: ftd::TextSource::Caption,
                                    },
                                },
                            ),
                            (
                                s("no-of-employees"),
                                ftd::PropertyValue::Value {
                                    value: ftd::Value::Integer { value: 50 },
                                },
                            ),
                        ])
                        .collect(),
                    },
                },
                conditions: vec![],
            }),
        );
        bag.insert(
            s("foo/bar#amitu"),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: s("amitu"),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::OrType {
                        name: s("foo/bar#lead"),
                        variant: s("individual"),
                        fields: std::array::IntoIter::new([
                            (
                                s("name"),
                                ftd::PropertyValue::Value {
                                    value: ftd::Value::String {
                                        text: s("Amit Upadhyay"),
                                        source: ftd::TextSource::Caption,
                                    },
                                },
                            ),
                            (
                                s("phone"),
                                ftd::PropertyValue::Reference {
                                    name: s("foo/bar#default-phone"),
                                    kind: ftd::p2::Kind::string()
                                        .set_default(Some(s("$foo/bar#default-phone"))),
                                },
                            ),
                        ])
                        .collect(),
                    },
                },
                conditions: vec![],
            }),
        );
        bag.insert(
            s("foo/bar#default-phone"),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: s("default-phone"),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::String {
                        text: s("1000"),
                        source: ftd::TextSource::Caption,
                    },
                },
                conditions: vec![],
            }),
        );
        bag.insert(
            s("foo/bar#lead"),
            ftd::p2::Thing::OrType(ftd::OrType {
                name: s("foo/bar#lead"),
                variants: vec![
                    ftd::p2::Record {
                        name: s("foo/bar#lead.individual"),
                        fields: std::array::IntoIter::new([
                            (s("name"), ftd::p2::Kind::caption()),
                            (
                                s("phone"),
                                ftd::p2::Kind::string()
                                    .set_default(Some(s("$foo/bar#default-phone"))),
                            ),
                        ])
                        .collect(),
                        instances: Default::default(),
                        order: vec![s("name"), s("phone")],
                    },
                    ftd::p2::Record {
                        name: s("foo/bar#lead.company"),
                        fields: std::array::IntoIter::new([
                            (
                                s("contact"),
                                ftd::p2::Kind::string().set_default(Some(s("1001"))),
                            ),
                            (s("fax"), ftd::p2::Kind::string()),
                            (s("name"), ftd::p2::Kind::caption()),
                            (
                                s("no-of-employees"),
                                ftd::p2::Kind::integer().set_default(Some(s("50"))),
                            ),
                        ])
                        .collect(),
                        instances: Default::default(),
                        order: vec![s("name"), s("contact"), s("fax"), s("no-of-employees")],
                    },
                ],
            }),
        );

        let (g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- string default-phone: 1000

                -- or-type lead:

                --- individual:
                caption name:
                string phone: $default-phone

                --- company:
                caption name:
                string contact: 1001
                string fax:
                integer no-of-employees: 50

                -- lead.individual amitu: Amit Upadhyay

                -- lead.company acme: Acme Inc.
                contact: John Doe
                fax: +1-234-567890

                -- ftd.text: $amitu.name

                -- ftd.text: $amitu.phone

                -- ftd.text: $acme.contact
                size: $acme.no-of-employees

                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        pretty_assertions::assert_eq!(g_bag, bag);
        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn default_id() {
        let mut main = super::default_column();

        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![ftd::Element::Column(ftd::Column {
                        spacing: None,
                        container: ftd::Container {
                            children: vec![ftd::Element::Row(ftd::Row {
                                spacing: None,
                                container: ftd::Container {
                                    children: vec![ftd::Element::Column(ftd::Column {
                                        spacing: None,
                                        container: ftd::Container {
                                            children: vec![ftd::Element::Markup(ftd::Markups {
                                                text: ftd::markdown_line("hello"),
                                                line: true,
                                                ..Default::default()
                                            })],
                                            ..Default::default()
                                        },
                                        common: ftd::Common {
                                            data_id: Some(s("display-text-id")),
                                            ..Default::default()
                                        },
                                    })],
                                    ..Default::default()
                                },
                                ..Default::default()
                            })],
                            ..Default::default()
                        },
                        common: ftd::Common {
                            data_id: Some(s("inside-page-id")),
                            ..Default::default()
                        },
                    })],
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![
                        ftd::Element::Column(ftd::Column {
                            spacing: None,
                            container: ftd::Container {
                                children: vec![ftd::Element::Row(ftd::Row {
                                    spacing: None,
                                    container: ftd::Container {
                                        children: vec![ftd::Element::Column(ftd::Column {
                                            spacing: None,
                                            container: ftd::Container {
                                                children: vec![ftd::Element::Markup(
                                                    ftd::Markups {
                                                        text: ftd::markdown_line("hello"),
                                                        line: true,
                                                        ..Default::default()
                                                    },
                                                )],
                                                ..Default::default()
                                            },
                                            common: ftd::Common {
                                                data_id: Some(s("display-text-id")),
                                                id: Some(s(
                                                    "page-id:inside-page-id:display-text-id",
                                                )),
                                                ..Default::default()
                                            },
                                        })],
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                })],
                                ..Default::default()
                            },
                            common: ftd::Common {
                                data_id: Some(s("inside-page-id")),
                                id: Some(s("page-id:inside-page-id")),
                                ..Default::default()
                            },
                        }),
                        ftd::Element::Row(ftd::Row {
                            spacing: None,
                            common: ftd::Common {
                                data_id: Some(s("page-id-row")),
                                id: Some(s("page-id-row")),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                    ],
                    ..Default::default()
                },
                common: ftd::Common {
                    data_id: Some(s("page-id")),
                    id: Some(s("page-id")),
                    ..Default::default()
                },
            }));

        main.container.children.push(ftd::Element::Row(ftd::Row {
            spacing: None,
            ..Default::default()
        }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- ftd.column display-text:

                --- ftd.text: hello


                -- ftd.column inside-page:

                --- ftd.row:

                --- display-text:
                id: display-text-id


                -- ftd.column page:

                --- inside-page:
                id: inside-page-id

                -- page:

                -- page:
                id: page-id

                -- ftd.row:

                -- container: page-id

                -- ftd.row:
                id: page-id-row

                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn region_h1() {
        let mut main = super::default_column();

        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("Heading 31"),
                        line: true,
                        common: ftd::Common {
                            region: Some(ftd::Region::Title),
                            reference: Some(s("@title@0")),
                            ..Default::default()
                        },
                        ..Default::default()
                    })],
                    ..Default::default()
                },
                common: ftd::Common {
                    locals: std::array::IntoIter::new([(s("title@0"), s("Heading 31"))]).collect(),
                    region: Some(ftd::Region::H3),
                    id: Some(s("heading-31")),
                    ..Default::default()
                },
            }));

        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("Heading 11"),
                            line: true,
                            common: ftd::Common {
                                region: Some(ftd::Region::Title),
                                reference: Some(s("@title@1")),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                        ftd::Element::Column(ftd::Column {
                            spacing: None,
                            container: ftd::Container {
                                children: vec![
                                    ftd::Element::Markup(ftd::Markups {
                                        text: ftd::markdown_line("Heading 21"),
                                        line: true,
                                        common: ftd::Common {
                                            region: Some(ftd::Region::Title),
                                            reference: Some(s("@title@2")),
                                            ..Default::default()
                                        },
                                        ..Default::default()
                                    }),
                                    ftd::Element::Column(ftd::Column {
                                        spacing: None,
                                        container: ftd::Container {
                                            children: vec![
                                                ftd::Element::Markup(ftd::Markups {
                                                    text: ftd::markdown_line("Heading 32"),
                                                    line: true,
                                                    common: ftd::Common {
                                                        region: Some(ftd::Region::Title),
                                                        reference: Some(s("@title@3")),
                                                        ..Default::default()
                                                    },
                                                    ..Default::default()
                                                }),
                                                ftd::Element::Markup(ftd::Markups {
                                                    text: ftd::markdown_line("hello"),
                                                    line: true,
                                                    ..Default::default()
                                                }),
                                            ],
                                            ..Default::default()
                                        },
                                        common: ftd::Common {
                                            locals: std::array::IntoIter::new([(
                                                s("title@3"),
                                                s("Heading 32"),
                                            )])
                                            .collect(),
                                            region: Some(ftd::Region::H3),
                                            id: Some(s("heading-32")),
                                            ..Default::default()
                                        },
                                    }),
                                ],
                                ..Default::default()
                            },
                            common: ftd::Common {
                                locals: std::array::IntoIter::new([(
                                    s("title@2"),
                                    s("Heading 21"),
                                )])
                                .collect(),
                                region: Some(ftd::Region::H2),
                                id: Some(s("heading-21")),
                                ..Default::default()
                            },
                        }),
                        ftd::Element::Column(ftd::Column {
                            spacing: None,
                            container: ftd::Container {
                                children: vec![ftd::Element::Markup(ftd::Markups {
                                    text: ftd::markdown_line("Heading 22"),
                                    line: true,
                                    common: ftd::Common {
                                        reference: Some(s("@title@5")),
                                        region: Some(ftd::Region::Title),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                })],
                                ..Default::default()
                            },
                            common: ftd::Common {
                                locals: std::array::IntoIter::new([(
                                    s("title@5"),
                                    s("Heading 22"),
                                )])
                                .collect(),
                                region: Some(ftd::Region::H2),
                                id: Some(s("heading-22")),
                                ..Default::default()
                            },
                        }),
                        ftd::Element::Column(ftd::Column {
                            spacing: None,
                            container: ftd::Container {
                                children: vec![ftd::Element::Markup(ftd::Markups {
                                    text: ftd::markdown_line("Heading 23"),
                                    line: true,
                                    common: ftd::Common {
                                        region: Some(ftd::Region::Title),
                                        reference: Some(s("@title@6")),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                })],
                                ..Default::default()
                            },
                            common: ftd::Common {
                                locals: std::array::IntoIter::new([(
                                    s("title@6"),
                                    s("Heading 23"),
                                )])
                                .collect(),
                                region: Some(ftd::Region::H2),
                                id: Some(s("heading-23")),
                                ..Default::default()
                            },
                        }),
                    ],
                    ..Default::default()
                },
                common: ftd::Common {
                    locals: std::array::IntoIter::new([(s("title@1"), s("Heading 11"))]).collect(),
                    region: Some(ftd::Region::H1),
                    id: Some(s("heading-11")),
                    ..Default::default()
                },
            }));

        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("Heading 12"),
                            line: true,
                            common: ftd::Common {
                                reference: Some(s("@title@7")),
                                region: Some(ftd::Region::Title),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                        ftd::Element::Column(ftd::Column {
                            spacing: None,
                            container: ftd::Container {
                                children: vec![ftd::Element::Markup(ftd::Markups {
                                    text: ftd::markdown_line("Heading 33"),
                                    line: true,
                                    common: ftd::Common {
                                        reference: Some(s("@title@8")),
                                        region: Some(ftd::Region::Title),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                })],
                                ..Default::default()
                            },
                            common: ftd::Common {
                                locals: std::array::IntoIter::new([(
                                    s("title@8"),
                                    s("Heading 33"),
                                )])
                                .collect(),
                                region: Some(ftd::Region::H3),
                                id: Some(s("heading-33")),
                                ..Default::default()
                            },
                        }),
                        ftd::Element::Column(ftd::Column {
                            spacing: None,
                            container: ftd::Container {
                                children: vec![ftd::Element::Markup(ftd::Markups {
                                    text: ftd::markdown_line("Heading 24"),
                                    line: true,
                                    common: ftd::Common {
                                        reference: Some(s("@title@9")),
                                        region: Some(ftd::Region::Title),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                })],
                                ..Default::default()
                            },
                            common: ftd::Common {
                                locals: std::array::IntoIter::new([(
                                    s("title@9"),
                                    s("Heading 24"),
                                )])
                                .collect(),
                                region: Some(ftd::Region::H2),
                                id: Some(s("heading-24")),
                                ..Default::default()
                            },
                        }),
                    ],
                    ..Default::default()
                },
                common: ftd::Common {
                    locals: std::array::IntoIter::new([(s("title@7"), s("Heading 12"))]).collect(),
                    region: Some(ftd::Region::H1),
                    id: Some(s("heading-12")),
                    ..Default::default()
                },
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- ftd.column h1:
                region: h1
                caption title:

                --- ftd.text:
                text: $title
                caption title:
                region: title

                -- ftd.column h2:
                region: h2
                caption title:

                --- ftd.text:
                text: $title
                caption title:
                region: title

                -- ftd.column h3:
                region: h3
                caption title:

                --- ftd.text:
                text: $title
                caption title:
                region: title

                -- h3: Heading 31

                -- h1: Heading 11

                -- h2: Heading 21

                -- h3: Heading 32

                -- ftd.text: hello

                -- h2: Heading 22

                -- h2: Heading 23

                -- h1: Heading 12

                -- h3: Heading 33

                -- h2: Heading 24

                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn event_onclick() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("Mobile"),
                            line: true,
                            common: ftd::Common {
                                condition: Some(ftd::Condition {
                                    variable: s("foo/bar#mobile"),
                                    value: s("true"),
                                }),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("Desktop"),
                            line: true,
                            common: ftd::Common {
                                condition: Some(ftd::Condition {
                                    variable: s("foo/bar#mobile"),
                                    value: s("false"),
                                }),
                                is_not_visible: true,
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                    ],
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("Click Here!"),
                line: true,
                common: ftd::Common {
                    events: vec![ftd::Event {
                        name: s("onclick"),
                        action: ftd::Action {
                            action: s("toggle"),
                            target: s("foo/bar#mobile"),
                            parameters: Default::default(),
                        },
                    }],
                    ..Default::default()
                },
                ..Default::default()
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- boolean mobile: true

                -- ftd.column foo:

                --- ftd.text: Mobile
                if: $mobile

                --- ftd.text: Desktop
                if: not $mobile

                -- foo:

                -- ftd.text: Click Here!
                $event-click$: toggle $mobile
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn event_toggle_with_local_variable() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("Hello"),
                line: true,
                common: ftd::Common {
                    locals: std::array::IntoIter::new([
                        (s("name@0"), s("Hello")),
                        (s("open@0"), s("true")),
                    ])
                    .collect(),
                    reference: Some(s("@name@0")),
                    condition: Some(ftd::Condition {
                        variable: s("@open@0"),
                        value: s("true"),
                    }),
                    events: vec![ftd::Event {
                        name: s("onclick"),
                        action: ftd::Action {
                            action: s("toggle"),
                            target: s("@open@0"),
                            parameters: Default::default(),
                        },
                    }],
                    ..Default::default()
                },
                ..Default::default()
            }));

        let mut bag = super::default_bag();
        bag.insert(
            s("foo/bar#foo"),
            ftd::p2::Thing::Component(ftd::Component {
                root: "ftd.text".to_string(),
                full_name: "foo/bar#foo".to_string(),
                arguments: std::array::IntoIter::new([
                    (s("name"), ftd::p2::Kind::caption()),
                    (
                        s("open"),
                        ftd::p2::Kind::boolean().set_default(Some(s("true"))),
                    ),
                ])
                .collect(),
                properties: std::array::IntoIter::new([(
                    s("text"),
                    ftd::component::Property {
                        default: Some(ftd::PropertyValue::Variable {
                            name: s("name"),
                            kind: ftd::p2::Kind::String {
                                caption: true,
                                body: true,
                                default: None,
                            },
                        }),
                        ..Default::default()
                    },
                )])
                .collect(),
                instructions: vec![],
                events: vec![ftd::p2::Event {
                    name: ftd::p2::EventName::OnClick,
                    action: ftd::p2::Action {
                        action: ftd::p2::ActionKind::Toggle,
                        target: s("@open"),
                        parameters: Default::default(),
                    },
                }],
                condition: Some(ftd::p2::Boolean::Equal {
                    left: ftd::PropertyValue::Variable {
                        name: s("open"),
                        kind: ftd::p2::Kind::boolean().set_default(Some(s("true"))),
                    },
                    right: ftd::PropertyValue::Value {
                        value: ftd::variable::Value::Boolean { value: true },
                    },
                }),
                kernel: false,
                invocations: vec![std::array::IntoIter::new([
                    (
                        s("name"),
                        ftd::Value::String {
                            text: s("Hello"),
                            source: ftd::TextSource::Caption,
                        },
                    ),
                    (s("open"), ftd::Value::Boolean { value: true }),
                ])
                .collect()],
                line_number: 1,
                ..Default::default()
            }),
        );

        let (g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- ftd.text foo:
                caption name:
                boolean open: true
                text: $name
                if: $open
                $event-click$: toggle $open

                -- foo: Hello
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        pretty_assertions::assert_eq!(g_col, main);
        pretty_assertions::assert_eq!(g_bag, bag);
    }

    #[test]
    fn event_toggle_with_local_variable_for_component() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("Click here"),
                            line: true,
                            common: ftd::Common {
                                events: vec![ftd::Event {
                                    name: s("onclick"),
                                    action: ftd::Action {
                                        action: s("toggle"),
                                        target: s("@open@0"),
                                        parameters: Default::default(),
                                    },
                                }],
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("Open True"),
                            line: true,
                            common: ftd::Common {
                                condition: Some(ftd::Condition {
                                    variable: s("@open@0"),
                                    value: s("true"),
                                }),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("Open False"),
                            line: true,
                            common: ftd::Common {
                                condition: Some(ftd::Condition {
                                    variable: s("@open@0"),
                                    value: s("false"),
                                }),
                                is_not_visible: true,
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                    ],
                    ..Default::default()
                },
                common: ftd::Common {
                    locals: std::array::IntoIter::new([(s("open@0"), s("true"))]).collect(),
                    ..Default::default()
                },
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- ftd.column foo:
                boolean open: true

                --- ftd.text: Click here
                $event-click$: toggle $open

                --- ftd.text: Open True
                if: $open

                --- ftd.text: Open False
                if: not $open

                -- foo:
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn event_toggle_for_loop() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("ab title"),
                            line: true,
                            common: ftd::Common {
                                events: vec![ftd::Event {
                                    name: s("onclick"),
                                    action: ftd::Action {
                                        action: s("toggle"),
                                        target: s("@open@0"),
                                        parameters: Default::default(),
                                    },
                                }],
                                reference: Some(s("@toc.title")),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                        ftd::Element::Column(ftd::Column {
                            spacing: None,
                            container: ftd::Container {
                                children: vec![ftd::Element::Markup(ftd::Markups {
                                    text: ftd::markdown_line("aa title"),
                                    line: true,
                                    common: ftd::Common {
                                        events: vec![ftd::Event {
                                            name: s("onclick"),
                                            action: ftd::Action {
                                                action: s("toggle"),
                                                target: s("@open@0,1"),
                                                parameters: Default::default(),
                                            },
                                        }],
                                        reference: Some(s("@toc.title")),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                })],
                                ..Default::default()
                            },
                            common: ftd::Common {
                                locals: std::array::IntoIter::new([
                                    (s("open@0,1"), s("true")),
                                    (s("toc@0,1"), s("{\"title\":\"aa title\"}")),
                                ])
                                .collect(),
                                condition: Some(ftd::Condition {
                                    variable: s("@open@0"),
                                    value: s("true"),
                                }),
                                ..Default::default()
                            },
                        }),
                        ftd::Element::Column(ftd::Column {
                            spacing: None,
                            container: ftd::Container {
                                children: vec![ftd::Element::Markup(ftd::Markups {
                                    text: ftd::markdown_line("aaa title"),
                                    line: true,
                                    common: ftd::Common {
                                        events: vec![ftd::Event {
                                            name: s("onclick"),
                                            action: ftd::Action {
                                                action: s("toggle"),
                                                target: s("@open@0,2"),
                                                parameters: Default::default(),
                                            },
                                        }],
                                        reference: Some(s("@toc.title")),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                })],
                                ..Default::default()
                            },
                            common: ftd::Common {
                                locals: std::array::IntoIter::new([
                                    (s("open@0,2"), s("true")),
                                    (s("toc@0,2"), s("{\"title\":\"aaa title\"}")),
                                ])
                                .collect(),
                                condition: Some(ftd::Condition {
                                    variable: s("@open@0"),
                                    value: s("true"),
                                }),
                                ..Default::default()
                            },
                        }),
                    ],
                    ..Default::default()
                },
                common: ftd::Common {
                    locals: std::array::IntoIter::new([
                        (s("open@0"), s("true")),
                        (s("toc@0"), s("{\"title\":\"ab title\"}")),
                    ])
                    .collect(),
                    reference: Some(s("foo/bar#toc")),
                    ..Default::default()
                },
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- record toc-record:
                string title:
                toc-record list children:

                -- ftd.column toc-item:
                toc-record toc:
                boolean open: true

                --- ftd.text: $toc.title
                $event-click$: toggle $open

                --- toc-item:
                if: $open
                $loop$: $toc.children as $obj
                toc: $obj

                -- toc-record list aa:

                -- aa:
                title: aa title

                -- aa:
                title: aaa title

                -- toc-record list toc:

                -- toc:
                title: ab title
                children: $aa

                -- toc-item:
                $loop$: $toc as $obj
                toc: $obj
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn test_local_variable() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![ftd::Element::Column(ftd::Column {
                        spacing: None,
                        container: ftd::Container {
                            children: vec![
                                ftd::Element::Column(ftd::Column {
                                    spacing: None,
                                    container: ftd::Container {
                                        children: vec![
                                            ftd::Element::Markup(ftd::Markups {
                                                text: ftd::markdown_line("Click here!"),
                                                line: true,
                                                common: ftd::Common {
                                                    events: vec![ftd::Event {
                                                        name: s("onclick"),
                                                        action: ftd::Action {
                                                            action: s("toggle"),
                                                            target: s("@open@0"),
                                                            parameters: Default::default(),
                                                        },
                                                    }],
                                                    ..Default::default()
                                                },
                                                ..Default::default()
                                            }),
                                            ftd::Element::Markup(ftd::Markups {
                                                text: ftd::markdown_line("Hello"),
                                                line: true,
                                                ..Default::default()
                                            }),
                                        ],
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                }),
                                ftd::Element::Column(ftd::Column {
                                    spacing: None,
                                    container: ftd::Container {
                                        children: vec![ftd::Element::Markup(ftd::Markups {
                                            text: ftd::markdown_line("Hello Bar"),
                                            line: true,
                                            ..Default::default()
                                        })],
                                        ..Default::default()
                                    },
                                    common: ftd::Common {
                                        locals: std::array::IntoIter::new([(
                                            s("open-bar@0,0,1"),
                                            s("true"),
                                        )])
                                        .collect(),
                                        condition: Some(ftd::Condition {
                                            variable: s("@open@0"),
                                            value: s("true"),
                                        }),
                                        ..Default::default()
                                    },
                                }),
                            ],
                            ..Default::default()
                        },
                        common: ftd::Common {
                            data_id: Some(s("foo-id")),
                            ..Default::default()
                        },
                    })],
                    ..Default::default()
                },
                common: ftd::Common {
                    locals: std::array::IntoIter::new([(s("open@0"), s("true"))]).collect(),
                    ..Default::default()
                },
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- ftd.column bar:
                boolean open-bar: true

                --- ftd.text: Hello Bar


                -- ftd.column foo:
                boolean open: true

                --- ftd.column:
                id: foo-id

                --- ftd.column:

                --- ftd.text: Click here!
                $event-click$: toggle $open

                --- ftd.text: Hello

                --- container: foo-id

                --- bar:
                if: $open


                -- foo:
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");
        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn if_on_var_integer() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Integer(ftd::Text {
                text: markdown_line("20"),
                common: ftd::Common {
                    reference: Some(s("foo/bar#bar")),
                    ..Default::default()
                },
                ..Default::default()
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- boolean foo: false

                -- integer bar: 10

                -- bar: 20
                if: not $foo

                -- ftd.integer:
                value: $bar

                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn if_on_var_text() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: markdown_line("other-foo says hello"),
                line: true,
                common: ftd::Common {
                    reference: Some(s("foo/bar#bar")),
                    ..Default::default()
                },
                ..Default::default()
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- boolean foo: false

                -- boolean other-foo: true

                -- string bar: hello

                -- bar: foo says hello
                if: not $foo

                -- bar: other-foo says hello
                if: $other-foo

                -- ftd.text: $bar

                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn cursor_pointer() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: markdown_line("hello"),
                line: true,
                common: ftd::Common {
                    cursor: Some(s("pointer")),
                    ..Default::default()
                },
                ..Default::default()
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- ftd.text: hello
                cursor: pointer

                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn comments() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("hello2"),
                line: true,
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("/hello3"),
                line: true,
                common: ftd::Common {
                    color: Some(ftd::Color {
                        r: 255,
                        g: 0,
                        b: 0,
                        alpha: 1.0,
                    }),
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container.children.push(ftd::Element::Row(ftd::Row {
            spacing: None,
            container: ftd::Container {
                children: vec![ftd::Element::Markup(ftd::Markups {
                    text: ftd::markdown_line("hello5"),
                    line: true,
                    common: ftd::Common {
                        color: Some(ftd::Color {
                            r: 0,
                            g: 128,
                            b: 0,
                            alpha: 1.0,
                        }),
                        ..Default::default()
                    },
                    ..Default::default()
                })],
                ..Default::default()
            },
            ..Default::default()
        }));

        main.container.children.push(ftd::Element::Row(ftd::Row {
            spacing: None,
            container: ftd::Container {
                children: vec![ftd::Element::Markup(ftd::Markups {
                    text: ftd::markdown_line("/foo says hello"),
                    line: true,
                    ..Default::default()
                })],
                ..Default::default()
            },
            ..Default::default()
        }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                r"
                /-- ftd.text:
                cursor: pointer

                hello1

                -- ftd.text:
                /color: red

                hello2

                -- ftd.text:
                color: red

                \/hello3

                -- ftd.row:

                /--- ftd.text: hello4

                --- ftd.text: hello5
                color: green
                /padding-left: 20

                -- ftd.row foo:
                /color: red

                --- ftd.text:

                \/foo says hello

                /--- ftd.text: foo says hello again

                -- foo:

                /-- foo:
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn component_declaration_anywhere_2() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![
                        ftd::Element::Column(ftd::Column {
                            spacing: None,
                            container: ftd::Container {
                                children: vec![
                                    ftd::Element::Markup(ftd::Markups {
                                        text: ftd::markdown_line("Bar says hello"),
                                        line: true,
                                        common: ftd::Common {
                                            reference: Some(s("@name@0,0")),
                                            ..Default::default()
                                        },
                                        ..Default::default()
                                    }),
                                    ftd::Element::Markup(ftd::Markups {
                                        text: ftd::markdown_line("Hello"),
                                        line: true,
                                        common: ftd::Common {
                                            reference: Some(s("foo/bar#greeting")),
                                            ..Default::default()
                                        },
                                        ..Default::default()
                                    }),
                                ],
                                ..Default::default()
                            },
                            common: ftd::Common {
                                locals: std::array::IntoIter::new([(
                                    s("name@0,0"),
                                    s("Bar says hello"),
                                )])
                                .collect(),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("foo says hello"),
                            line: true,
                            ..Default::default()
                        }),
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("Hello"),
                            line: true,
                            common: ftd::Common {
                                reference: Some(s("foo/bar#greeting")),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                    ],
                    ..Default::default()
                },
                ..Default::default()
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- foo:

                -- ftd.column foo:

                --- bar: Bar says hello

                --- ftd.text: foo says hello

                --- ftd.text: $greeting

                -- string greeting: Hello

                -- ftd.column bar:
                caption name:

                --- ftd.text: $name

                --- ftd.text: $greeting
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn action_increment_decrement_condition_1() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Integer(ftd::Text {
                text: ftd::markdown_line("0"),
                common: ftd::Common {
                    reference: Some(s("foo/bar#count")),
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("Hello on 8"),
                line: true,
                common: ftd::Common {
                    condition: Some(ftd::Condition {
                        variable: s("foo/bar#count"),
                        value: s("8"),
                    }),
                    is_not_visible: true,
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("increment counter"),
                line: true,
                common: ftd::Common {
                    events: vec![ftd::Event {
                        name: s("onclick"),
                        action: ftd::Action {
                            action: s("increment"),
                            target: s("foo/bar#count"),
                            parameters: Default::default(),
                        },
                    }],
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("decrement counter"),
                line: true,
                common: ftd::Common {
                    events: vec![ftd::Event {
                        name: s("onclick"),
                        action: ftd::Action {
                            action: s("decrement"),
                            target: s("foo/bar#count"),
                            parameters: Default::default(),
                        },
                    }],
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("increment counter"),
                line: true,
                common: ftd::Common {
                    events: vec![ftd::Event {
                        name: s("onclick"),
                        action: ftd::Action {
                            action: s("increment"),
                            target: s("foo/bar#count"),
                            parameters: std::array::IntoIter::new([(
                                s("by"),
                                vec![ftd::event::ParameterData {
                                    value: "2".to_string(),
                                    reference: None,
                                }],
                            )])
                            .collect(),
                        },
                    }],
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("increment counter by 2 clamp 2 10"),
                line: true,
                common: ftd::Common {
                    events: vec![ftd::Event {
                        name: s("onclick"),
                        action: ftd::Action {
                            action: s("increment"),
                            target: s("foo/bar#count"),
                            parameters: std::array::IntoIter::new([
                                (
                                    s("by"),
                                    vec![ftd::event::ParameterData {
                                        value: "2".to_string(),
                                        reference: None,
                                    }],
                                ),
                                (
                                    s("clamp"),
                                    vec![
                                        ftd::event::ParameterData {
                                            value: "2".to_string(),
                                            reference: None,
                                        },
                                        ftd::event::ParameterData {
                                            value: "10".to_string(),
                                            reference: None,
                                        },
                                    ],
                                ),
                            ])
                            .collect(),
                        },
                    }],
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("decrement count clamp 2 10"),
                line: true,
                common: ftd::Common {
                    events: vec![ftd::Event {
                        name: s("onclick"),
                        action: ftd::Action {
                            action: s("decrement"),
                            target: s("foo/bar#count"),
                            parameters: std::array::IntoIter::new([(
                                s("clamp"),
                                vec![
                                    ftd::event::ParameterData {
                                        value: "2".to_string(),
                                        reference: None,
                                    },
                                    ftd::event::ParameterData {
                                        value: "10".to_string(),
                                        reference: None,
                                    },
                                ],
                            )])
                            .collect(),
                        },
                    }],
                    ..Default::default()
                },
                ..Default::default()
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- integer count: 0

                -- ftd.integer:
                value: $count

                -- ftd.text: Hello on 8
                if: $count == 8

                -- ftd.text: increment counter
                $event-click$: increment $count

                -- ftd.text: decrement counter
                $event-click$: decrement $count

                -- ftd.text: increment counter
                $event-click$: increment $count by 2

                -- ftd.text: increment counter by 2 clamp 2 10
                $event-click$: increment $count by 2 clamp 2 10

                -- ftd.text: decrement count clamp 2 10
                $event-click$: decrement $count clamp 2 10
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");
        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn action_increment_decrement_local_variable() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![
                        ftd::Element::Integer(ftd::Text {
                            text: ftd::markdown_line("0"),
                            common: ftd::Common {
                                reference: Some(s("@count@0")),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("increment counter"),
                            line: true,
                            common: ftd::Common {
                                events: vec![ftd::Event {
                                    name: s("onclick"),
                                    action: ftd::Action {
                                        action: s("increment"),
                                        target: s("@count@0"),
                                        parameters: std::array::IntoIter::new([(
                                            s("by"),
                                            vec![ftd::event::ParameterData {
                                                value: "3".to_string(),
                                                reference: None,
                                            }],
                                        )])
                                        .collect(),
                                    },
                                }],
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("decrement counter"),
                            line: true,
                            common: ftd::Common {
                                events: vec![ftd::Event {
                                    name: s("onclick"),
                                    action: ftd::Action {
                                        action: s("decrement"),
                                        target: s("@count@0"),
                                        parameters: std::array::IntoIter::new([(
                                            s("by"),
                                            vec![ftd::event::ParameterData {
                                                value: "2".to_string(),
                                                reference: Some(s("foo/bar#decrement-by")),
                                            }],
                                        )])
                                        .collect(),
                                    },
                                }],
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                    ],
                    ..Default::default()
                },
                common: ftd::Common {
                    locals: std::array::IntoIter::new([
                        (s("by@0"), s("3")),
                        (s("count@0"), s("0")),
                    ])
                    .collect(),
                    ..Default::default()
                },
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- integer decrement-by: 2

                -- ftd.column foo:
                integer by: 4
                integer count: 0

                --- ftd.integer:
                value: $count

                --- ftd.text: increment counter
                $event-click$: increment $count by $by

                --- ftd.text: decrement counter
                $event-click$: decrement $count by $decrement-by

                -- foo:
                by: 3

                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");
        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn nested_component() {
        let mut main = super::default_column();
        main.container.children.push(ftd::Element::Row(ftd::Row {
            spacing: None,
            common: ftd::Common {
                locals: std::array::IntoIter::new([(s("cta@0"), s("CTA says Hello"))]).collect(),
                ..Default::default()
            },
            ..Default::default()
        }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- secondary-button: CTA says Hello

                -- secondary-button-1 secondary-button:
                caption cta:
                cta: $cta


                -- ftd.row secondary-button-1:
                caption cta:

                --- ftd.text: $cta
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn action_increment_decrement_on_component() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Image(ftd::Image {
                src: s("https://www.liveabout.com/thmb/YCJmu1khSJo8kMYM090QCd9W78U=/1250x0/filters:no_upscale():max_bytes(150000):strip_icc():format(webp)/powerpuff_girls-56a00bc45f9b58eba4aea61d.jpg"),
                common: ftd::Common {
                    condition: Some(
                        ftd::Condition {
                            variable: s("foo/bar#count"),
                            value: s("0"),
                        },
                    ),
                    is_not_visible: false,
                    events: vec![
                        ftd::Event {
                            name: s("onclick"),
                            action: ftd::Action {
                                action: s("increment"),
                                target: s("foo/bar#count"),
                                parameters: std::array::IntoIter::new([(s("clamp"), vec![ftd::event::ParameterData {
                                    value: "0".to_string(),
                                    reference: None,
                                }, ftd::event::ParameterData {
                                    value: "1".to_string(),
                                    reference: None,
                                }])])
                                    .collect(),
                            },
                        },
                    ],
                    locals: std::array::IntoIter::new([
                        (s("idx@0"), s("0")),
                        (s("src@0"), s("https://www.liveabout.com/thmb/YCJmu1khSJo8kMYM090QCd9W78U=/1250x0/filters:no_upscale():max_bytes(150000):strip_icc():format(webp)/powerpuff_girls-56a00bc45f9b58eba4aea61d.jpg"))
                    ]).collect(),
                    reference: Some(s("@src@0")),
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Image(ftd::Image {
                src: s("https://upload.wikimedia.org/wikipedia/en/d/d4/Mickey_Mouse.png"),
                common: ftd::Common {
                    condition: Some(ftd::Condition {
                        variable: s("foo/bar#count"),
                        value: s("1"),
                    }),
                    is_not_visible: true,
                    events: vec![ftd::Event {
                        name: s("onclick"),
                        action: ftd::Action {
                            action: s("increment"),
                            target: s("foo/bar#count"),
                            parameters: std::array::IntoIter::new([(
                                s("clamp"),
                                vec![
                                    ftd::event::ParameterData {
                                        value: "0".to_string(),
                                        reference: None,
                                    },
                                    ftd::event::ParameterData {
                                        value: "1".to_string(),
                                        reference: None,
                                    },
                                ],
                            )])
                            .collect(),
                        },
                    }],
                    locals: std::array::IntoIter::new([
                        (s("idx@1"), s("1")),
                        (
                            s("src@1"),
                            s("https://upload.wikimedia.org/wikipedia/en/d/d4/Mickey_Mouse.png"),
                        ),
                    ])
                    .collect(),
                    reference: Some(s("@src@1")),
                    ..Default::default()
                },
                ..Default::default()
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- integer count: 0

                -- ftd.image slide:
                string src:
                integer idx:
                src: $src
                if: $count == $idx
                $event-click$: increment $count clamp 0 1

                -- slide:
                src: https://www.liveabout.com/thmb/YCJmu1khSJo8kMYM090QCd9W78U=/1250x0/filters:no_upscale():max_bytes(150000):strip_icc():format(webp)/powerpuff_girls-56a00bc45f9b58eba4aea61d.jpg
                idx: 0

                -- slide:
                src: https://upload.wikimedia.org/wikipedia/en/d/d4/Mickey_Mouse.png
                idx: 1
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn loop_on_list_string() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("Arpita"),
                            line: true,
                            ..Default::default()
                        }),
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("Ayushi"),
                            line: true,
                            ..Default::default()
                        }),
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("AmitU"),
                            line: true,
                            ..Default::default()
                        }),
                    ],
                    ..Default::default()
                },
                ..Default::default()
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- ftd.column foo:
                string list bar:

                --- ftd.text: $obj
                $loop$: $bar as $obj

                -- string list names:

                -- names: Arpita

                -- names: Ayushi

                -- names: AmitU

                -- foo:
                bar: $names
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn open_container_with_parent_id() {
        let mut main = super::default_column();
        let beverage_external_children = vec![ftd::Element::Column(ftd::Column {
            spacing: None,
            container: ftd::Container {
                children: vec![
                    ftd::Element::Column(ftd::Column {
                        spacing: None,
                        container: ftd::Container {
                            children: vec![
                                ftd::Element::Markup(ftd::Markups {
                                    text: ftd::markdown_line("Water"),
                                    line: true,
                                    common: ftd::Common {
                                        events: vec![ftd::Event {
                                            name: s("onclick"),
                                            action: ftd::Action {
                                                action: s("toggle"),
                                                target: s("@visible@0,0,0"),
                                                ..Default::default()
                                            },
                                        }],
                                        reference: Some(s("@name@0,0,0")),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                }),
                                ftd::Element::Column(ftd::Column {
                                    spacing: None,
                                    common: ftd::Common {
                                        condition: Some(ftd::Condition {
                                            variable: s("@visible@0,0,0"),
                                            value: s("true"),
                                        }),
                                        data_id: Some(s("some-child")),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                }),
                            ],
                            external_children: Some((s("some-child"), vec![vec![1]], vec![])),
                            open: (None, Some(s("some-child"))),
                            ..Default::default()
                        },
                        common: ftd::Common {
                            locals: std::array::IntoIter::new([
                                (s("name@0,0,0"), s("Water")),
                                (s("visible@0,0,0"), s("true")),
                            ])
                            .collect(),
                            ..Default::default()
                        },
                    }),
                    ftd::Element::Column(ftd::Column {
                        spacing: None,
                        container: ftd::Container {
                            children: vec![
                                ftd::Element::Markup(ftd::Markups {
                                    text: ftd::markdown_line("Juice"),
                                    line: true,
                                    common: ftd::Common {
                                        events: vec![ftd::Event {
                                            name: s("onclick"),
                                            action: ftd::Action {
                                                action: s("toggle"),
                                                target: s("@visible@0,0,1"),
                                                ..Default::default()
                                            },
                                        }],
                                        reference: Some(s("@name@0,0,1")),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                }),
                                ftd::Element::Column(ftd::Column {
                                    spacing: None,
                                    common: ftd::Common {
                                        condition: Some(ftd::Condition {
                                            variable: s("@visible@0,0,1"),
                                            value: s("true"),
                                        }),
                                        data_id: Some(s("some-child")),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                }),
                            ],
                            external_children: Some((
                                s("some-child"),
                                vec![vec![1]],
                                vec![ftd::Element::Column(ftd::Column {
                                    spacing: None,
                                    container: ftd::Container {
                                        children: vec![ftd::Element::Column(ftd::Column {
                                            spacing: None,
                                            container: ftd::Container {
                                                children: vec![
                                                    ftd::Element::Markup(ftd::Markups {
                                                        text: ftd::markdown_line("Mango Juice"),
                                                        line: true,
                                                        common: ftd::Common {
                                                            events: vec![ftd::Event {
                                                                name: s("onclick"),
                                                                action: ftd::Action {
                                                                    action: s("toggle"),
                                                                    target: s("@visible@0,0,1,0"),
                                                                    ..Default::default()
                                                                },
                                                            }],
                                                            reference: Some(s("@name@0,0,1,0")),
                                                            ..Default::default()
                                                        },
                                                        ..Default::default()
                                                    }),
                                                    ftd::Element::Column(ftd::Column {
                                                        spacing: None,
                                                        common: ftd::Common {
                                                            condition: Some(ftd::Condition {
                                                                variable: s("@visible@0,0,1,0"),
                                                                value: s("true"),
                                                            }),
                                                            data_id: Some(s("some-child")),
                                                            ..Default::default()
                                                        },
                                                        ..Default::default()
                                                    }),
                                                ],
                                                external_children: Some((
                                                    s("some-child"),
                                                    vec![vec![1]],
                                                    vec![],
                                                )),
                                                open: (None, Some(s("some-child"))),
                                                ..Default::default()
                                            },
                                            common: ftd::Common {
                                                locals: std::array::IntoIter::new([
                                                    (s("name@0,0,1,0"), s("Mango Juice")),
                                                    (s("visible@0,0,1,0"), s("true")),
                                                ])
                                                .collect(),
                                                ..Default::default()
                                            },
                                        })],
                                        ..Default::default()
                                    },
                                    common: ftd::Common {
                                        width: Some(ftd::Length::Fill),
                                        height: Some(ftd::Length::Fill),
                                        position: Some(ftd::Position::Center),
                                        ..Default::default()
                                    },
                                })],
                            )),
                            open: (None, Some(s("some-child"))),
                            ..Default::default()
                        },
                        common: ftd::Common {
                            locals: std::array::IntoIter::new([
                                (s("name@0,0,1"), s("Juice")),
                                (s("visible@0,0,1"), s("true")),
                            ])
                            .collect(),
                            ..Default::default()
                        },
                    }),
                ],
                ..Default::default()
            },
            common: ftd::Common {
                width: Some(ftd::Length::Fill),
                height: Some(ftd::Length::Fill),
                position: Some(ftd::Position::Center),
                ..Default::default()
            },
        })];

        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![ftd::Element::Column(ftd::Column {
                        spacing: None,
                        container: ftd::Container {
                            children: vec![
                                ftd::Element::Markup(ftd::Markups {
                                    text: ftd::markdown_line("Beverage"),
                                    line: true,
                                    common: ftd::Common {
                                        events: vec![ftd::Event {
                                            name: s("onclick"),
                                            action: ftd::Action {
                                                action: s("toggle"),
                                                target: s("@visible@0,0"),
                                                ..Default::default()
                                            },
                                        }],
                                        reference: Some(s("@name@0,0")),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                }),
                                ftd::Element::Column(ftd::Column {
                                    spacing: None,
                                    common: ftd::Common {
                                        condition: Some(ftd::Condition {
                                            variable: s("@visible@0,0"),
                                            value: s("true"),
                                        }),
                                        data_id: Some(s("some-child")),
                                        id: Some(s("beverage:some-child")),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                }),
                            ],
                            external_children: Some((
                                s("some-child"),
                                vec![vec![1]],
                                beverage_external_children,
                            )),
                            open: (None, Some(s("some-child"))),
                            ..Default::default()
                        },
                        common: ftd::Common {
                            locals: std::array::IntoIter::new([
                                (s("name@0,0"), s("Beverage")),
                                (s("visible@0,0"), s("true")),
                            ])
                            .collect(),
                            data_id: Some(s("beverage")),
                            id: Some(s("beverage")),
                            ..Default::default()
                        },
                    })],
                    ..Default::default()
                },
                ..Default::default()
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
            -- ftd.column display-item1:
            string name:
            open: some-child
            boolean visible: true

            --- ftd.text: $name
            $event-click$: toggle $visible

            --- ftd.column:
            if: $visible
            id: some-child

            -- ftd.column:

            -- display-item1:
            name: Beverage
            id: beverage


            -- display-item1:
            name: Water


            -- container: beverage


            -- display-item1:
            name: Juice


            -- display-item1:
            name: Mango Juice
            "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn text_check() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("$hello"),
                            line: true,
                            ..Default::default()
                        }),
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("hello"),
                            line: true,
                            common: ftd::Common {
                                reference: Some(s("@hello2@0")),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("hello"),
                            line: true,
                            common: ftd::Common {
                                reference: Some(s("foo/bar#hello")),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("hello"),
                            line: true,
                            ..Default::default()
                        }),
                    ],
                    ..Default::default()
                },
                common: ftd::Common {
                    locals: std::array::IntoIter::new([(s("hello2@0"), s("hello"))]).collect(),
                    ..Default::default()
                },
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                r"
                -- string hello: hello

                -- ftd.column foo:
                string hello2:

                --- ftd.text: \$hello

                --- ftd.text: $hello2

                --- ftd.text: $hello

                --- ftd.text: hello

                -- foo:
                hello2: $hello
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn caption() {
        let mut main = super::default_column();

        main.container
            .children
            .push(ftd::Element::Integer(ftd::Text {
                text: ftd::markdown_line("32"),
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Boolean(ftd::Text {
                text: ftd::markdown_line("true"),
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Decimal(ftd::Text {
                text: ftd::markdown_line("0.06"),
                ..Default::default()
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- ftd.integer: 32

                -- ftd.boolean: true

                -- ftd.decimal: 0.06
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn heading_id() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("Heading 00"),
                            line: true,
                            common: ftd::Common {
                                region: Some(ftd::Region::Title),
                                reference: Some(s("@title@0")),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("Heading 00 body"),
                            line: true,
                            common: ftd::Common {
                                id: Some(s("one:markdown-id")),
                                data_id: Some(s("markdown-id")),
                                locals: std::array::IntoIter::new([(
                                    s("body@0,1"),
                                    s("Heading 00 body"),
                                )])
                                .collect(),
                                reference: Some(s("@body@0,1")),
                                condition: Some(ftd::Condition {
                                    variable: s("@body@0"),
                                    value: s("$IsNotNull$"),
                                }),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                    ],
                    ..Default::default()
                },
                common: ftd::Common {
                    region: Some(ftd::Region::H0),
                    id: Some(s("one")),
                    data_id: Some(s("one")),
                    locals: std::array::IntoIter::new([
                        (s("body@0"), s("Heading 00 body")),
                        (s("title@0"), s("Heading 00")),
                    ])
                    .collect(),
                    ..Default::default()
                },
            }));

        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("Heading 01"),
                            line: true,
                            common: ftd::Common {
                                region: Some(ftd::Region::Title),
                                reference: Some(s("@title@1")),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("Heading 01 body"),
                            line: true,
                            common: ftd::Common {
                                data_id: Some(s("markdown-id")),
                                locals: std::array::IntoIter::new([(
                                    s("body@1,1"),
                                    s("Heading 01 body"),
                                )])
                                .collect(),
                                condition: Some(ftd::Condition {
                                    variable: s("@body@1"),
                                    value: s("$IsNotNull$"),
                                }),
                                reference: Some(s("@body@1,1")),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                    ],
                    ..Default::default()
                },
                common: ftd::Common {
                    region: Some(ftd::Region::H0),
                    id: Some(s("heading-01")),
                    locals: std::array::IntoIter::new([
                        (s("body@1"), s("Heading 01 body")),
                        (s("title@1"), s("Heading 01")),
                    ])
                    .collect(),
                    ..Default::default()
                },
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- h0: Heading 00
                id: one

                Heading 00 body

                -- h0: Heading 01

                Heading 01 body

                -- ftd.column h0:
                caption title:
                optional body body:
                region: h0

                --- ftd.text:
                text: $title
                region: title

                --- markdown:
                if: $body is not null
                body: $body
                id: markdown-id

                -- ftd.text markdown:
                body body:
                text: $body
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn new_id() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("hello"),
                        line: true,
                        common: ftd::Common {
                            data_id: Some(s("hello")),
                            ..Default::default()
                        },
                        ..Default::default()
                    })],
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("hello"),
                        line: true,
                        common: ftd::Common {
                            data_id: Some(s("hello")),
                            id: Some(s("asd:hello")),
                            ..Default::default()
                        },
                        ..Default::default()
                    })],
                    ..Default::default()
                },
                common: ftd::Common {
                    data_id: Some(s("asd")),
                    id: Some(s("asd")),
                    ..Default::default()
                },
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
            --  ftd.column foo:

            --- ftd.text: hello
            id: hello

            -- foo:

            -- foo:
            id: asd
            "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn list_is_empty_check() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("Hello people"),
                line: true,
                ..Default::default()
            }));

        main.container.children.push(ftd::Element::Null);

        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![
                        ftd::Element::Null,
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("Hello empty list"),
                            line: true,
                            ..Default::default()
                        }),
                    ],
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("Hello list"),
                            line: true,
                            ..Default::default()
                        }),
                        ftd::Element::Null,
                    ],
                    ..Default::default()
                },
                ..Default::default()
            }));
        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- string list people:

                -- people: Ayushi

                -- people: Arpita

                -- ftd.text: Hello people
                if: $people is not empty

                -- ftd.text: Hello nobody
                if: $people is empty


                -- string list empty-list:


                -- ftd.column foo:
                string list string-list:

                --- ftd.text: Hello list
                if: $string-list is not empty

                --- ftd.text: Hello empty list
                if: $string-list is empty

                -- foo:
                string-list: $empty-list

                -- foo:
                string-list: $people
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");
        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn parent_with_unsatisfied_condition() {
        let mut main = super::default_column();
        main.container.children.push(ftd::Element::Null);
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("Hello"),
                        line: true,
                        ..Default::default()
                    })],
                    ..Default::default()
                },
                common: ftd::Common {
                    is_not_visible: true,
                    ..Default::default()
                },
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- string list empty-list:

                -- ftd.column:
                if: $empty-list is not empty

                --- ftd.text: Hello

                -- foo:

                -- ftd.column foo:
                if: $empty-list is not empty

                --- ftd.text: Hello
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn open_container_id_with_children() {
        let mut external_children = super::default_column();
        external_children
            .container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("hello"),
                line: true,
                ..Default::default()
            }));
        external_children
            .container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("world"),
                line: true,
                ..Default::default()
            }));

        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![ftd::Element::Column(ftd::Column {
                        spacing: None,
                        common: ftd::Common {
                            id: Some(s("foo-id:some-id")),
                            data_id: Some(s("some-id")),
                            ..Default::default()
                        },
                        ..Default::default()
                    })],
                    external_children: Some((
                        s("some-id"),
                        vec![vec![0]],
                        vec![ftd::Element::Column(external_children)],
                    )),
                    open: (None, Some(s("some-id"))),
                    ..Default::default()
                },
                common: ftd::Common {
                    id: Some(s("foo-id")),
                    data_id: Some(s("foo-id")),
                    ..Default::default()
                },
            }));

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("Outside"),
                line: true,
                ..Default::default()
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- foo:
                id: foo-id

                --- ftd.text: hello

                --- ftd.text: world

                -- ftd.text: Outside


                -- ftd.column foo:
                open: some-id

                --- ftd.column:
                id: some-id
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn loop_record_list() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![
                        ftd::Element::Column(ftd::Column {
                            spacing: None,
                            container: ftd::Container {
                                children: vec![ftd::Element::Markup(ftd::Markups {
                                    text: ftd::markdown_line("commit message 1"),
                                    line: true,
                                    common: ftd::Common {
                                        reference: Some(s("@commit.message")),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                })],
                                ..Default::default()
                            },
                            common: ftd::Common {
                                locals: std::array::IntoIter::new([(
                                    s("commit@0,0"),
                                    s("{\"message\":\"commit message 1\"}"),
                                )])
                                .collect(),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                        ftd::Element::Column(ftd::Column {
                            spacing: None,
                            container: ftd::Container {
                                children: vec![ftd::Element::Markup(ftd::Markups {
                                    text: ftd::markdown_line("commit message 2"),
                                    line: true,
                                    common: ftd::Common {
                                        reference: Some(s("@commit.message")),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                })],
                                ..Default::default()
                            },
                            common: ftd::Common {
                                locals: std::array::IntoIter::new([(
                                    s("commit@0,1"),
                                    s("{\"message\":\"commit message 2\"}"),
                                )])
                                .collect(),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                        ftd::Element::Column(ftd::Column {
                            spacing: None,
                            container: ftd::Container {
                                children: vec![ftd::Element::Markup(ftd::Markups {
                                    text: ftd::markdown_line("file filename 1"),
                                    line: true,
                                    common: ftd::Common {
                                        reference: Some(s("@file.filename")),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                })],
                                ..Default::default()
                            },
                            common: ftd::Common {
                                locals: std::array::IntoIter::new([(
                                    s("file@0,2"),
                                    s("{\"filename\":\"file filename 1\"}"),
                                )])
                                .collect(),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                        ftd::Element::Column(ftd::Column {
                            spacing: None,
                            container: ftd::Container {
                                children: vec![ftd::Element::Markup(ftd::Markups {
                                    text: ftd::markdown_line("file filename 2"),
                                    line: true,
                                    common: ftd::Common {
                                        reference: Some(s("@file.filename")),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                })],
                                ..Default::default()
                            },
                            common: ftd::Common {
                                locals: std::array::IntoIter::new([(
                                    s("file@0,3"),
                                    s("{\"filename\":\"file filename 2\"}"),
                                )])
                                .collect(),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                    ],
                    ..Default::default()
                },
                common: ftd::Common {
                    locals: std::array::IntoIter::new([(s("changes@0"), s("{}"))]).collect(),
                    ..Default::default()
                },
                ..Default::default()
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- record commit:
                string message:

                -- record file:
                string filename:

                -- record changes:
                commit list commits:
                file list files:


                -- commit list commit-list:

                -- commit-list:
                message: commit message 1

                -- commit-list:
                message: commit message 2


                -- file list file-list:

                -- file-list:
                filename: file filename 1

                -- file-list:
                filename: file filename 2


                -- changes rec-changes:
                commits: $commit-list
                files: $file-list

                -- display:
                changes: $rec-changes


                -- ftd.column display:
                changes changes:

                --- display-commit:
                $loop$: $changes.commits as $obj
                commit: $obj

                --- display-file:
                $loop$: $changes.files as $obj
                file: $obj


                -- ftd.column display-commit:
                commit commit:

                --- ftd.text: $commit.message


                -- ftd.column display-file:
                file file:

                --- ftd.text: $file.filename
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");
        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn scene_children_with_default_position() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Scene(ftd::Scene {
                spacing: None,
                container: ftd::Container {
                    children: vec![ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("Hello"),
                        line: true,
                        common: ftd::Common {
                            top: Some(0),
                            left: Some(0),
                            ..Default::default()
                        },
                        ..Default::default()
                    }), ftd::Element::Markup(ftd::Markups {
                        text: ftd::markdown_line("World"),
                        line: true,
                        common: ftd::Common {
                            top: Some(10),
                            right: Some(30),
                            scale: Some(1.5),
                            scale_x: Some(-1.0),
                            scale_y: Some(-1.0),
                            rotate: Some(45),
                            position: Some(ftd::Position::Center),
                            ..Default::default()
                        },
                        ..Default::default()
                    })],
                    ..Default::default()
                },
                common: ftd::Common {
                    width: Some(
                        ftd::Length::Px {
                            value: 1000,
                        },
                    ),
                    background_image: Some(
                        s("https://image.shutterstock.com/z/stock-&lt;!&ndash;&ndash;&gt;vector-vector-illustration-of-a-beautiful-summer-landscape-143054302.jpg"),
                    ),
                    ..Default::default()
                }
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- ftd.scene:
                background-image: https://image.shutterstock.com/z/stock-&lt;!&ndash;&ndash;&gt;vector-vector-illustration-of-a-beautiful-summer-landscape-143054302.jpg
                width: 1000

                --- ftd.text: Hello

                --- foo:
                top: 10
                right: 30
                align: center
                scale: 1.5
                rotate: 45
                scale-x: -1
                scale-y: -1

                -- ftd.text foo:
                text: World
                "
            ),
            &ftd::p2::TestLibrary {},
        )
            .expect("found error");

        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn event_set() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("Start..."),
                line: true,
                common: ftd::Common {
                    condition: Some(ftd::Condition {
                        variable: s("foo/bar#current"),
                        value: s("some value"),
                    }),
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("some value"),
                line: true,
                common: ftd::Common {
                    reference: Some(s("foo/bar#current")),
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("change message"),
                line: true,
                common: ftd::Common {
                    events: vec![ftd::Event {
                        name: s("onclick"),
                        action: ftd::Action {
                            action: s("set-value"),
                            target: s("foo/bar#current"),
                            parameters: std::array::IntoIter::new([(
                                s("value"),
                                vec![
                                    ftd::event::ParameterData {
                                        value: s("hello world"),
                                        reference: None,
                                    },
                                    ftd::event::ParameterData {
                                        value: s("string"),
                                        reference: None,
                                    },
                                ],
                            )])
                            .collect(),
                        },
                    }],
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("change message again"),
                line: true,
                common: ftd::Common {
                    events: vec![ftd::Event {
                        name: s("onclick"),
                        action: ftd::Action {
                            action: s("set-value"),
                            target: s("foo/bar#current"),
                            parameters: std::array::IntoIter::new([(
                                s("value"),
                                vec![
                                    ftd::event::ParameterData {
                                        value: s("good bye"),
                                        reference: Some(s("foo/bar#msg")),
                                    },
                                    ftd::event::ParameterData {
                                        value: s("string"),
                                        reference: None,
                                    },
                                ],
                            )])
                            .collect(),
                        },
                    }],
                    ..Default::default()
                },
                ..Default::default()
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- string current: some value

                -- ftd.text: Start...
                if: $current == some value

                -- ftd.text: $current

                -- ftd.text: change message
                $event-click$: $current = hello world

                -- string msg: good bye

                -- ftd.text: change message again
                $event-click$: $current = $msg
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");
        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn absolute_positioning() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("hello world"),
                line: true,
                common: ftd::Common {
                    anchor: Some(ftd::Anchor::Parent),
                    right: Some(0),
                    top: Some(100),
                    ..Default::default()
                },
                ..Default::default()
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- ftd.text: hello world
                anchor: parent
                right: 0
                top: 100
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");
        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn inherit_check() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("hello"),
                line: true,
                size: Some(50),
                common: ftd::Common {
                    locals: std::array::IntoIter::new([(s("size@0"), s("50"))]).collect(),
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("hello"),
                line: true,
                common: ftd::Common {
                    locals: std::array::IntoIter::new([(s("size@1"), s(""))]).collect(),
                    ..Default::default()
                },
                ..Default::default()
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- ftd.text foo: hello
                inherit size:

                -- foo:
                size: 50

                -- foo:
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");
        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn inner_container_check() {
        let mut main = super::default_column();
        let col = ftd::Element::Column(ftd::Column {
            spacing: None,
            container: ftd::Container {
                children: vec![ftd::Element::Column(ftd::Column {
                    spacing: None,
                    container: ftd::Container {
                        children: vec![
                            ftd::Element::Image(ftd::Image {
                                src: s("https://www.nilinswap.com/static/img/dp.jpeg"),
                                ..Default::default()
                            }),
                            ftd::Element::Markup(ftd::Markups {
                                text: ftd::markdown_line("Swapnil Sharma"),
                                line: true,
                                ..Default::default()
                            }),
                        ],
                        ..Default::default()
                    },
                    ..Default::default()
                })],
                ..Default::default()
            },
            ..Default::default()
        });
        main.container.children.push(col.clone());
        main.container.children.push(col);

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- ftd.column:

                --- ftd.column:

                --- ftd.image:
                src: https://www.nilinswap.com/static/img/dp.jpeg

                --- ftd.text: Swapnil Sharma


                -- ftd.column foo:

                --- ftd.column:

                --- ftd.image:
                src: https://www.nilinswap.com/static/img/dp.jpeg

                --- ftd.text: Swapnil Sharma

                -- foo:
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");
        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn mouse_in() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("Hello World"),
                line: true,
                common: ftd::Common {
                    locals: std::array::IntoIter::new([(s("MOUSE-IN@0"), s("false"))]).collect(),
                    conditional_attribute: std::array::IntoIter::new([(
                        s("color"),
                        ftd::ConditionalAttribute {
                            attribute_type: ftd::AttributeType::Style,
                            conditions_with_value: vec![(
                                ftd::Condition {
                                    variable: s("@MOUSE-IN@0"),
                                    value: s("true"),
                                },
                                ftd::ConditionalValue {
                                    value: s("rgba(255,0,0,1)"),
                                    important: false,
                                },
                            )],
                            default: None,
                        },
                    )])
                    .collect(),
                    events: vec![
                        ftd::Event {
                            name: s("onmouseenter"),
                            action: ftd::Action {
                                action: s("set-value"),
                                target: s("@MOUSE-IN@0"),
                                parameters: std::array::IntoIter::new([(
                                    s("value"),
                                    vec![
                                        ftd::event::ParameterData {
                                            value: s("true"),
                                            reference: None,
                                        },
                                        ftd::event::ParameterData {
                                            value: s("boolean"),
                                            reference: None,
                                        },
                                    ],
                                )])
                                .collect(),
                            },
                        },
                        ftd::Event {
                            name: s("onmouseleave"),
                            action: ftd::Action {
                                action: s("set-value"),
                                target: s("@MOUSE-IN@0"),
                                parameters: std::array::IntoIter::new([(
                                    s("value"),
                                    vec![
                                        ftd::event::ParameterData {
                                            value: s("false"),
                                            reference: None,
                                        },
                                        ftd::event::ParameterData {
                                            value: s("boolean"),
                                            reference: None,
                                        },
                                    ],
                                )])
                                .collect(),
                            },
                        },
                    ],
                    ..Default::default()
                },
                ..Default::default()
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- ftd.text foo:
                text: Hello World
                color if $MOUSE-IN: red

                -- foo:
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");
        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn event_stop_propagation() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("Hello"),
                            line: true,
                            common: ftd::Common {
                                condition: Some(ftd::Condition {
                                    variable: s("@open@0"),
                                    value: s("true"),
                                }),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                        ftd::Element::Column(ftd::Column {
                            spacing: None,
                            container: ftd::Container {
                                children: vec![ftd::Element::Markup(ftd::Markups {
                                    text: ftd::markdown_line("Hello Again"),
                                    line: true,
                                    common: ftd::Common {
                                        condition: Some(ftd::Condition {
                                            variable: s("@open@0,1"),
                                            value: s("true"),
                                        }),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                })],
                                ..Default::default()
                            },
                            common: ftd::Common {
                                locals: std::array::IntoIter::new([(s("open@0,1"), s("true"))])
                                    .collect(),
                                events: vec![
                                    ftd::Event {
                                        name: s("onclick"),
                                        action: ftd::Action {
                                            action: s("toggle"),
                                            target: s("@open@0,1"),
                                            parameters: Default::default(),
                                        },
                                    },
                                    ftd::Event {
                                        name: s("onclick"),
                                        action: ftd::Action {
                                            action: s("stop-propagation"),
                                            target: s(""),
                                            parameters: Default::default(),
                                        },
                                    },
                                ],
                                ..Default::default()
                            },
                        }),
                    ],
                    ..Default::default()
                },
                common: ftd::Common {
                    locals: std::array::IntoIter::new([(s("open@0"), s("true"))]).collect(),
                    events: vec![ftd::Event {
                        name: s("onclick"),
                        action: ftd::Action {
                            action: s("toggle"),
                            target: s("@open@0"),
                            parameters: Default::default(),
                        },
                    }],
                    ..Default::default()
                },
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- foo:

                -- ftd.column foo:
                boolean open: true
                $event-click$: toggle $open

                --- ftd.text: Hello
                if: $open

                --- bar:


                -- ftd.column bar:
                boolean open: true
                $event-click$: toggle $open
                $event-click$: stop-propagation

                --- ftd.text: Hello Again
                if: $open

                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");
        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn new_syntax() {
        let mut main = super::default_column();
        main.container.children.push(ftd::Element::Row(ftd::Row {
            spacing: None,
            container: ftd::Container {
                children: vec![ftd::Element::Integer(ftd::Text {
                    text: ftd::markdown_line("20"),
                    common: ftd::Common {
                        conditional_attribute: std::array::IntoIter::new([(
                            s("color"),
                            ftd::ConditionalAttribute {
                                attribute_type: ftd::AttributeType::Style,
                                conditions_with_value: vec![
                                    (
                                        ftd::Condition {
                                            variable: s("@b@0"),
                                            value: s("true"),
                                        },
                                        ftd::ConditionalValue {
                                            value: s("rgba(0,0,0,1)"),
                                            important: false,
                                        },
                                    ),
                                    (
                                        ftd::Condition {
                                            variable: s("@a@0"),
                                            value: s("30"),
                                        },
                                        ftd::ConditionalValue {
                                            value: s("rgba(255,0,0,1)"),
                                            important: false,
                                        },
                                    ),
                                ],
                                default: None,
                            },
                        )])
                        .collect(),
                        reference: Some(s("@a@0")),
                        ..Default::default()
                    },
                    ..Default::default()
                })],
                ..Default::default()
            },
            common: ftd::Common {
                locals: std::array::IntoIter::new([(s("a@0"), s("20")), (s("b@0"), s("false"))])
                    .collect(),
                events: vec![
                    ftd::Event {
                        name: s("onclick"),
                        action: ftd::Action {
                            action: s("toggle"),
                            target: s("@b@0"),
                            parameters: Default::default(),
                        },
                    },
                    ftd::Event {
                        name: s("onclick"),
                        action: ftd::Action {
                            action: s("increment"),
                            target: s("@a@0"),
                            parameters: std::array::IntoIter::new([(
                                s("by"),
                                vec![ftd::event::ParameterData {
                                    value: s("2"),
                                    reference: None,
                                }],
                            )])
                            .collect(),
                        },
                    },
                ],
                ..Default::default()
            },
        }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- ftd.row foo:
                integer a:
                boolean b: false
                $event-click$: toggle $b
                $event-click$: increment $a by 2

                --- ftd.integer:
                value: $a
                color if $b: black
                color if $a == 30: red

                -- foo:
                a: 20
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");
        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn condition_check() {
        let mut main = super::default_column();
        main.container.children.push(ftd::Element::Row(ftd::Row {
            spacing: None,
            container: ftd::Container {
                children: vec![ftd::Element::Column(ftd::Column {
                    spacing: None,
                    container: ftd::Container {
                        children: vec![ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("Hello"),
                            line: true,
                            common: ftd::Common {
                                condition: Some(ftd::Condition {
                                    variable: s("@b@0,0"),
                                    value: s("true"),
                                }),
                                is_not_visible: true,
                                ..Default::default()
                            },
                            ..Default::default()
                        })],
                        ..Default::default()
                    },
                    common: ftd::Common {
                        locals: std::array::IntoIter::new([
                            (s("a@0,0"), s("true")),
                            (s("b@0,0"), s("false")),
                        ])
                        .collect(),
                        condition: Some(ftd::Condition {
                            variable: s("@b@0"),
                            value: s("true"),
                        }),
                        ..Default::default()
                    },
                })],
                ..Default::default()
            },
            common: ftd::Common {
                locals: std::array::IntoIter::new([(s("b@0"), s("true"))]).collect(),
                ..Default::default()
            },
        }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- boolean present: true

                -- ftd.column bar:
                boolean a: true
                if: $a
                boolean b: false

                --- ftd.text: Hello
                if: $b

                -- ftd.row foo:
                boolean b: true

                --- bar:
                if: $b

                -- foo:
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");
        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn external_variable() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![
                        ftd::Element::Integer(ftd::Text {
                            text: ftd::markdown_line("20"),
                            common: ftd::Common {
                                conditional_attribute: std::array::IntoIter::new([(
                                    s("color"),
                                    ftd::ConditionalAttribute {
                                        attribute_type: ftd::AttributeType::Style,
                                        conditions_with_value: vec![(
                                            ftd::Condition {
                                                variable: s("@b@0"),
                                                value: s("true"),
                                            },
                                            ftd::ConditionalValue {
                                                value: s("rgba(0,0,0,1)"),
                                                important: false,
                                            },
                                        )],
                                        default: None,
                                    },
                                )])
                                .collect(),
                                reference: Some(s("@a@0")),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("whatever"),
                            line: true,
                            common: ftd::Common {
                                reference: Some(s("@some-text@0")),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                    ],
                    ..Default::default()
                },
                common: ftd::Common {
                    locals: std::array::IntoIter::new([
                        (s("a@0"), s("20")),
                        (s("b@0"), s("false")),
                        (s("some-text@0"), s("whatever")),
                    ])
                    .collect(),
                    events: vec![
                        ftd::Event {
                            name: s("onclick"),
                            action: ftd::Action {
                                action: s("toggle"),
                                target: s("@b@0"),
                                parameters: Default::default(),
                            },
                        },
                        ftd::Event {
                            name: s("onclick"),
                            action: ftd::Action {
                                action: s("increment"),
                                target: s("@a@0"),
                                parameters: Default::default(),
                            },
                        },
                        ftd::Event {
                            name: s("onclick"),
                            action: ftd::Action {
                                action: s("set-value"),
                                target: s("@some-text@0"),
                                parameters: std::array::IntoIter::new([(
                                    "value".to_string(),
                                    vec![
                                        ftd::event::ParameterData {
                                            value: s("hello"),
                                            reference: Some(s("foo/bar#current")),
                                        },
                                        ftd::event::ParameterData {
                                            value: s("string"),
                                            reference: None,
                                        },
                                    ],
                                )])
                                .collect(),
                            },
                        },
                    ],
                    ..Default::default()
                },
            }));

        main.container.children.push(ftd::Element::Row(ftd::Row {
            spacing: None,
            container: ftd::Container {
                children: vec![ftd::Element::Markup(ftd::Markups {
                    text: ftd::markdown_line("hello"),
                    line: true,
                    common: ftd::Common {
                        conditional_attribute: std::array::IntoIter::new([(
                            s("color"),
                            ftd::ConditionalAttribute {
                                attribute_type: ftd::AttributeType::Style,
                                conditions_with_value: vec![(
                                    ftd::Condition {
                                        variable: s("@foo@1"),
                                        value: s("true"),
                                    },
                                    ftd::ConditionalValue {
                                        value: s("rgba(255,0,0,1)"),
                                        important: false,
                                    },
                                )],
                                default: None,
                            },
                        )])
                        .collect(),
                        ..Default::default()
                    },
                    ..Default::default()
                })],
                ..Default::default()
            },
            common: ftd::Common {
                locals: std::array::IntoIter::new([(s("foo@1"), s("false"))]).collect(),
                events: vec![ftd::Event {
                    name: s("onclick"),
                    action: ftd::Action {
                        action: s("toggle"),
                        target: s("@foo@1"),
                        parameters: Default::default(),
                    },
                }],
                ..Default::default()
            },
        }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- ftd.column foo:
                integer a:
                boolean b: false
                $event-click$: toggle $b
                $event-click$: increment $a

                --- ftd.integer:
                value: $a
                color if $b: black

                -- string current: hello

                -- foo:
                a: 20
                string some-text: whatever
                $event-click$: $some-text = $current

                --- ftd.text: $some-text

                -- ftd.row:
                boolean foo: false
                $event-click$: toggle $foo

                --- ftd.text: hello
                color if $foo: red
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");
        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn new_var_syntax() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("hello"),
                line: true,
                size: Some(30),
                common: ftd::Common {
                    conditional_attribute: std::array::IntoIter::new([(
                        s("color"),
                        ftd::ConditionalAttribute {
                            attribute_type: ftd::AttributeType::Style,
                            conditions_with_value: vec![(
                                ftd::Condition {
                                    variable: s("@t@0"),
                                    value: s("true"),
                                },
                                ftd::ConditionalValue {
                                    value: s("rgba(255,0,0,1)"),
                                    important: false,
                                },
                            )],
                            default: None,
                        },
                    )])
                    .collect(),
                    locals: std::array::IntoIter::new([
                        (s("f@0"), s("hello")),
                        (s("t@0"), s("true")),
                    ])
                    .collect(),
                    reference: Some(s("foo/bar#bar")),
                    color: Some(ftd::Color {
                        r: 255,
                        g: 0,
                        b: 0,
                        alpha: 1.0,
                    }),
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("hello"),
                            line: true,
                            common: ftd::Common {
                                reference: Some(s("@ff@1")),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                        ftd::Element::Integer(ftd::Text {
                            text: ftd::markdown_line("20"),
                            common: ftd::Common {
                                reference: Some(s("@i@1")),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                    ],
                    ..Default::default()
                },
                common: ftd::Common {
                    locals: std::array::IntoIter::new([
                        (s("ff@1"), s("hello")),
                        (s("i@1"), s("20")),
                    ])
                    .collect(),
                    ..Default::default()
                },
            }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- ftd.column col:
                integer i:
                string ff: hello

                --- ftd.text: $ff

                --- ftd.integer: $i

                -- integer foo: 20

                -- foo: 30

                -- string bar: hello

                -- ftd.text: $bar
                boolean t: true
                string f: hello
                size: $foo
                color if $t: red

                -- col:
                i: 20
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");
        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn text_block() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::TextBlock(ftd::TextBlock {
                text: ftd::markdown_line("hello"),
                line: true,
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::TextBlock(ftd::TextBlock {
                text: ftd::markdown_line("hello"),
                line: true,
                ..Default::default()
            }));

        main.container.children.push(ftd::Element::Code(ftd::Code {
            text: ftd::code_with_theme(
                "This is text",
                "txt",
                ftd::render::DEFAULT_THEME,
                "foo/bar",
            )
            .unwrap(),
            ..Default::default()
        }));

        let (_g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- ftd.text-block: hello

                -- ftd.text-block b: hello

                -- b:

                -- ftd.code:

                This is text
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");
        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn variable_component() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                spacing: None,
                container: ftd::Container {
                    children: vec![
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("amitu"),
                            line: true,
                            ..Default::default()
                        }),
                        ftd::Element::Markup(ftd::Markups {
                            text: ftd::markdown_line("hello"),
                            line: true,
                            common: ftd::Common {
                                color: Some(ftd::Color {
                                    r: 255,
                                    g: 0,
                                    b: 0,
                                    alpha: 1.0,
                                }),
                                locals: std::array::IntoIter::new([(s("size@0,1"), s("30"))])
                                    .collect(),
                                ..Default::default()
                            },
                            size: Some(30),
                            ..Default::default()
                        }),
                        ftd::Element::Column(ftd::Column {
                            spacing: None,
                            container: ftd::Container {
                                children: vec![
                                    ftd::Element::Markup(ftd::Markups {
                                        text: ftd::markdown_line("hello again"),
                                        line: true,
                                        common: ftd::Common {
                                            reference: Some(s("@msg@0,2")),
                                            ..Default::default()
                                        },
                                        ..Default::default()
                                    }),
                                    ftd::Element::Markup(ftd::Markups {
                                        text: ftd::markdown_line("hello world!"),
                                        line: true,
                                        common: ftd::Common {
                                            reference: Some(s("@other-msg@0,2")),
                                            ..Default::default()
                                        },
                                        ..Default::default()
                                    }),
                                    ftd::Element::Markup(ftd::Markups {
                                        text: ftd::markdown_line("hello"),
                                        line: true,
                                        common: ftd::Common {
                                            color: Some(ftd::Color {
                                                r: 255,
                                                g: 0,
                                                b: 0,
                                                alpha: 1.0,
                                            }),
                                            locals: std::array::IntoIter::new([(
                                                s("size@0,2,2"),
                                                s("20"),
                                            )])
                                            .collect(),
                                            ..Default::default()
                                        },
                                        size: Some(20),
                                        ..Default::default()
                                    }),
                                    ftd::Element::Markup(ftd::Markups {
                                        text: ftd::markdown_line("hello amitu!"),
                                        line: true,
                                        common: ftd::Common {
                                            color: Some(ftd::Color {
                                                r: 255,
                                                g: 0,
                                                b: 0,
                                                alpha: 1.0,
                                            }),
                                            ..Default::default()
                                        },
                                        size: Some(10),
                                        ..Default::default()
                                    }),
                                ],
                                ..Default::default()
                            },
                            common: ftd::Common {
                                locals: std::array::IntoIter::new([
                                    (s("msg@0,2"), s("hello again")),
                                    (s("other-msg@0,2"), s("hello world!")),
                                ])
                                .collect(),
                                ..Default::default()
                            },
                        }),
                    ],
                    ..Default::default()
                },
                ..Default::default()
            }));

        let (_g_bag, g_col) = crate::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- ftd.text foo: hello
                integer size: 10
                color: red
                size: $size

                -- ftd.column moo: 
                caption msg: world
                string other-msg: world again
                ftd.ui t:
                ftd.ui k:
                
                --- ftd.text: $msg

                --- ftd.text: $other-msg

                --- t:

                --- k:

                -- ftd.column bar:
                ftd.ui t: foo:
                > size: 30
                ftd.ui g:

                --- ftd.text: amitu

                --- t:

                --- g:

                -- bar:
                g: moo: hello again
                > other-msg: hello world!
                > t: foo:
                >> size: 20
                > k: ftd.text: hello amitu!
                >> color: red
                >> size: 10
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");
        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn optional_global_variable() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("hello"),
                line: true,
                common: ftd::Common {
                    reference: Some(s("foo/bar#active")),
                    condition: Some(ftd::Condition {
                        variable: s("foo/bar#active"),
                        value: s("$IsNotNull$"),
                    }),
                    ..Default::default()
                },
                ..Default::default()
            }));
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("Not Active"),
                line: true,
                common: ftd::Common {
                    condition: Some(ftd::Condition {
                        variable: s("foo/bar#active"),
                        value: s("$IsNull$"),
                    }),
                    is_not_visible: true,
                    ..Default::default()
                },
                ..Default::default()
            }));
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line(""),
                line: true,
                common: ftd::Common {
                    reference: Some(s("foo/bar#flags")),
                    condition: Some(ftd::Condition {
                        variable: s("foo/bar#flags"),
                        value: s("$IsNotNull$"),
                    }),
                    is_not_visible: true,
                    ..Default::default()
                },
                ..Default::default()
            }));
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("No Flag Available"),
                line: true,
                common: ftd::Common {
                    condition: Some(ftd::Condition {
                        variable: s("foo/bar#flags"),
                        value: s("$IsNull$"),
                    }),
                    ..Default::default()
                },
                ..Default::default()
            }));

        let (_g_bag, g_col) = crate::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- optional string active:

                -- active: hello
                
                -- ftd.text: $active
                if: $active is not null

                -- ftd.text: Not Active
                if: $active is null

                -- optional string flags:
                
                -- ftd.text: $flags
                if: $flags is not null

                -- ftd.text: No Flag Available
                if: $flags is null
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");
        pretty_assertions::assert_eq!(g_col, main);
    }

    #[test]
    fn object() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Column(ftd::Column {
                container: ftd::Container {
                    children: vec![ftd::Element::Markup(ftd::Markups {
                        text: ftd::markup_line("Data"),
                        line: true,
                        ..Default::default()
                    })],
                    ..Default::default()
                },
                common: ftd::Common {
                    locals: std::array::IntoIter::new([(
                        s("o@0"),
                        s("{\"a\":\"Madhav\",\"b\":\"bb\"}"),
                    )])
                    .collect(),
                    ..Default::default()
                },
                ..Default::default()
            }));

        let mut bag = super::default_bag();
        bag.insert(
            s("foo/bar#aa"),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: s("aa"),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::String {
                        text: s("Madhav"),
                        source: ftd::TextSource::Caption,
                    },
                },
                conditions: vec![],
            }),
        );
        bag.insert(
            s("foo/bar#foo"),
            ftd::p2::Thing::Component(ftd::Component {
                root: s("ftd.column"),
                full_name: s("foo/bar#foo"),
                arguments: std::array::IntoIter::new([(s("o"), ftd::p2::Kind::object())]).collect(),
                instructions: vec![ftd::Instruction::ChildComponent {
                    child: ftd::ChildComponent {
                        root: s("ftd#text"),
                        properties: std::array::IntoIter::new([(
                            s("text"),
                            ftd::component::Property {
                                default: Some(ftd::PropertyValue::Value {
                                    value: ftd::variable::Value::String {
                                        text: s("Data"),
                                        source: ftd::TextSource::Caption,
                                    },
                                }),
                                conditions: vec![],
                                nested_properties: Default::default(),
                            },
                        )])
                        .collect(),
                        ..Default::default()
                    },
                }],
                ..Default::default()
            }),
        );
        bag.insert(
            s("foo/bar#obj"),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: s("obj"),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::Object {
                        values: std::array::IntoIter::new([
                            (
                                s("a"),
                                ftd::PropertyValue::Reference {
                                    name: s("foo/bar#aa"),
                                    kind: ftd::p2::Kind::String {
                                        caption: true,
                                        body: false,
                                        default: None,
                                    },
                                },
                            ),
                            (
                                s("b"),
                                ftd::PropertyValue::Value {
                                    value: ftd::variable::Value::String {
                                        text: s("bb"),
                                        source: ftd::TextSource::Header,
                                    },
                                },
                            ),
                        ])
                        .collect(),
                    },
                },
                conditions: vec![],
            }),
        );

        p!(
            "
            -- string aa: Madhav

            -- object obj:
            a: $aa
            b: bb

            -- ftd.column foo:
            object o:
        
            --- ftd.text: Data

            -- foo:
            o: $obj
            ",
            (bag, main),
        );
    }

    #[test]
    fn event_change() {
        let mut main = super::default_column();
        let mut bag = super::default_bag();
        bag.insert(
            s("foo/bar#input-data"),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: s("input-data"),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::String {
                        text: s("Nothing"),
                        source: ftd::TextSource::Caption,
                    },
                },
                conditions: vec![],
            }),
        );

        bag.insert(
            s("foo/bar#obj"),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: s("obj"),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::Object {
                        values: std::array::IntoIter::new([
                            (
                                s("function"),
                                ftd::PropertyValue::Value {
                                    value: ftd::Value::String {
                                        text: s("some-function"),
                                        source: ftd::TextSource::Header,
                                    },
                                },
                            ),
                            (
                                s("value"),
                                ftd::PropertyValue::Reference {
                                    name: s("foo/bar#input-data"),
                                    kind: ftd::p2::Kind::String {
                                        caption: true,
                                        body: false,
                                        default: None,
                                    },
                                },
                            ),
                        ])
                        .collect(),
                    },
                },
                conditions: vec![],
            }),
        );

        main.container
            .children
            .push(ftd::Element::Input(ftd::Input {
                common: ftd::Common {
                    events: vec![
                        ftd::Event {
                            name: s("onchange"),
                            action: ftd::Action {
                                action: s("set-value"),
                                target: s("foo/bar#input-data"),
                                parameters: std::array::IntoIter::new([(
                                    s("value"),
                                    vec![
                                        ftd::event::ParameterData {
                                            value: s("$VALUE"),
                                            reference: None,
                                        },
                                        ftd::event::ParameterData {
                                            value: s("string"),
                                            reference: None,
                                        },
                                    ],
                                )])
                                .collect(),
                            },
                        },
                        ftd::Event {
                            name: s("onchange"),
                            action: ftd::Action {
                                action: s("message-host"),
                                target: s("$obj"),
                                parameters: std::array::IntoIter::new([(
                                    "data".to_string(),
                                    vec![ftd::event::ParameterData {
                                    value: s(
                                        "{\"function\":\"some-function\",\"value\":\"Nothing\"}",
                                    ),
                                    reference: Some(s("{\"value\":\"foo/bar#input-data\"}")),
                                }],
                                )])
                                .collect(),
                            },
                        },
                    ],
                    ..Default::default()
                },
                placeholder: None,
            }));

        p!(
            "
            -- string input-data: Nothing

            -- object obj:
            function: some-function
            value: $input-data

            -- ftd.input:
            $event-change$: $input-data=$VALUE
            $event-change$: message-host $obj
            ",
            (bag, main),
        );
    }

    #[test]
    fn component_processor() {
        let mut main = super::default_column();

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markup_line("Hello from text-component processor"),
                line: true,
                size: Some(40),
                ..Default::default()
            }));

        p!(
            "
            -- ftd.text: hello
            $processor$: text-component-processor
            ",
            (super::default_bag(), main),
        );
    }

    #[test]
    fn global_variable_pass_as_reference() {
        let mut main = super::default_column();
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("Arpita"),
                line: true,
                common: ftd::Common {
                    reference: Some(s("foo/bar#bar")),
                    ..Default::default()
                },
                ..Default::default()
            }));
        main.container
            .children
            .push(ftd::Element::Integer(ftd::Text {
                text: ftd::markdown_line("1"),
                common: ftd::Common {
                    reference: Some(s("foo/bar#ibar")),
                    ..Default::default()
                },
                ..Default::default()
            }));
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("Arpita"),
                line: true,
                common: ftd::Common {
                    reference: Some(s("foo/bar#lfoo")),
                    ..Default::default()
                },
                ..Default::default()
            }));
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("Arpita"),
                line: true,
                common: ftd::Common {
                    reference: Some(s("foo/bar#lfoo")),
                    ..Default::default()
                },
                ..Default::default()
            }));
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("Ayushi"),
                line: true,
                common: ftd::Common {
                    reference: Some(s("foo/bar#lfoo")),
                    ..Default::default()
                },
                ..Default::default()
            }));
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("$loop$"),
                line: true,
                common: ftd::Common {
                    is_dummy: true,
                    reference: Some(s("foo/bar#lfoo")),
                    ..Default::default()
                },
                ..Default::default()
            }));

        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("Arpita"),
                line: true,
                common: ftd::Common {
                    reference: Some(s("foo/bar#lbar")),
                    ..Default::default()
                },
                ..Default::default()
            }));
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("Arpita"),
                line: true,
                common: ftd::Common {
                    reference: Some(s("foo/bar#lbar")),
                    ..Default::default()
                },
                ..Default::default()
            }));
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("Ayushi"),
                line: true,
                common: ftd::Common {
                    reference: Some(s("foo/bar#lbar")),
                    ..Default::default()
                },
                ..Default::default()
            }));
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("$loop$"),
                line: true,
                common: ftd::Common {
                    is_dummy: true,
                    reference: Some(s("foo/bar#lbar")),
                    ..Default::default()
                },
                ..Default::default()
            }));
        main.container
            .children
            .push(ftd::Element::Markup(ftd::Markups {
                text: ftd::markdown_line("Arpita"),
                line: true,
                common: ftd::Common {
                    reference: Some(s("foo/bar#arpita.name")),
                    ..Default::default()
                },
                ..Default::default()
            }));

        let mut bag = super::default_bag();
        bag.insert(
            s("foo/bar#arpita"),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: s("arpita"),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::Record {
                        name: s("foo/bar#person"),
                        fields: std::array::IntoIter::new([(
                            s("name"),
                            ftd::PropertyValue::Reference {
                                name: s("foo/bar#bar"),
                                kind: ftd::p2::Kind::String {
                                    caption: true,
                                    body: false,
                                    default: None,
                                },
                            },
                        )])
                        .collect(),
                    },
                },
                conditions: vec![],
            }),
        );

        bag.insert(
            s("foo/bar#person"),
            ftd::p2::Thing::Record(ftd::p2::Record {
                name: s("foo/bar#person"),
                fields: std::array::IntoIter::new([(s("name"), ftd::p2::Kind::caption())])
                    .collect(),
                instances: Default::default(),
                order: vec![s("name")],
            }),
        );

        bag.insert(
            s("foo/bar#bar"),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: s("bar"),
                value: ftd::PropertyValue::Reference {
                    name: s("foo/bar#foo"),
                    kind: ftd::p2::Kind::String {
                        caption: true,
                        body: false,
                        default: None,
                    },
                },
                conditions: vec![],
            }),
        );

        bag.insert(
            s("foo/bar#foo"),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: s("foo"),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::String {
                        text: s("Arpita"),
                        source: ftd::TextSource::Caption,
                    },
                },
                conditions: vec![],
            }),
        );

        bag.insert(
            s("foo/bar#ibar"),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: s("ibar"),
                value: ftd::PropertyValue::Reference {
                    name: s("foo/bar#ifoo"),
                    kind: ftd::p2::Kind::Integer { default: None },
                },
                conditions: vec![],
            }),
        );

        bag.insert(
            s("foo/bar#ifoo"),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: s("ifoo"),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::Integer { value: 1 },
                },
                conditions: vec![],
            }),
        );

        bag.insert(
            s("foo/bar#lbar"),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: s("foo/bar#lbar"),
                value: ftd::PropertyValue::Reference {
                    name: s("foo/bar#lfoo"),
                    kind: ftd::p2::Kind::List {
                        kind: Box::new(ftd::p2::Kind::String {
                            caption: false,
                            body: false,
                            default: None,
                        }),
                        default: None,
                    },
                },
                conditions: vec![],
            }),
        );

        bag.insert(
            s("foo/bar#lfoo"),
            ftd::p2::Thing::Variable(ftd::Variable {
                flags: ftd::VariableFlags::default(),
                name: s("foo/bar#lfoo"),
                value: ftd::PropertyValue::Value {
                    value: ftd::Value::List {
                        data: vec![
                            ftd::PropertyValue::Reference {
                                name: s("foo/bar#foo"),
                                kind: ftd::p2::Kind::String {
                                    caption: true,
                                    body: false,
                                    default: None,
                                },
                            },
                            ftd::PropertyValue::Reference {
                                name: s("foo/bar#bar"),
                                kind: ftd::p2::Kind::String {
                                    caption: true,
                                    body: false,
                                    default: None,
                                },
                            },
                            ftd::PropertyValue::Value {
                                value: ftd::Value::String {
                                    text: s("Ayushi"),
                                    source: ftd::TextSource::Caption,
                                },
                            },
                        ],
                        kind: ftd::p2::Kind::String {
                            caption: false,
                            body: false,
                            default: None,
                        },
                    },
                },
                conditions: vec![],
            }),
        );

        let (g_bag, g_col) = crate::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- string foo: Arpita

                -- string bar: $foo

                -- integer ifoo: 1

                -- integer ibar: $ifoo

                -- string list lfoo:

                -- lfoo: $foo

                -- lfoo: $bar

                -- lfoo: Ayushi

                -- string list lbar: $lfoo

                -- record person:
                caption name:

                -- person arpita: $bar

                -- ftd.text: $bar

                -- ftd.integer: $ibar

                -- ftd.text: $obj
                $loop$: $lfoo as $obj

                -- ftd.text: $obj
                $loop$: $lbar as $obj

                -- ftd.text: $arpita.name
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        pretty_assertions::assert_eq!(g_col, main);
        pretty_assertions::assert_eq!(g_bag, bag);
    }

    #[test]
    fn locals_as_ref() {
        let (g_bag, g_col) = crate::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- string foo: Foo

                -- bar:
                title: $foo
                w: 2
                integer gg: 1

                --- ftd.integer: $gg

                -- bar:
                title: $foo
                w: 1

                -- ftd.column bar:
                string title:
                boolean active: false
                string subtitle: $foo
                string bio: $subtitle
                integer w:
                color: green
                border-width: $w
                
                --- ftd.text: $title

                --- ftd.text: $subtitle

                --- ftd.text: $bio
                
                --- ftd.boolean: $active 
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");

        dbg!(&g_bag, &g_col);
    }

    /*#[test]
    fn optional_condition_on_record() {
        let (_g_bag, g_col) = crate::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- record person-data:
                caption name:
                integer age:

                -- person-data person1: Madhav
                age: 10

                -- optional person-data person:

                -- ftd.text: $person.name
                if: $person is not null
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");
    }*/

    /*#[test]
    fn loop_with_tree_structure_1() {
        let (g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- record toc-record:
                title: string
                link: string
                children: list toc-record

                -- component toc-item:
                component: ftd.column
                toc-record $toc:
                padding-left: 10

                --- ftd.text: ref $toc.title
                link: ref $toc.link

                --- toc-item:
                $loop$: $toc.children as $obj
                toc: $obj


                -- toc-record list toc:

                -- toc:
                title: ref ab.title
                link: ref ab.link
                children: ref ab.children

                -- toc-record ab:
                title: ab title
                link: ab link

                -- ab.children first_ab
                title: aa title
                link: aa link

                --- children:
                title:

                -- ab.children:
                title: aaa title
                link: aaa link



                -- toc-item:
                $loop$: toc as $obj
                toc: $obj
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");
        // pretty_assertions::assert_eq!(g_bag, bag);
        // pretty_assertions::assert_eq!(g_col, main);
        // --- toc-item:
        //                 $loop$: $toc.children as $t
        //                 toc: $t
    }

    #[test]
    fn loop_with_tree_structure_2() {
        let (g_bag, g_col) = ftd::p2::interpreter::interpret(
            "foo/bar",
            indoc::indoc!(
                "
                -- record toc-record:
                title: string
                link: string
                children: list toc-record

                -- component toc-item:
                component: ftd.column
                toc-record $toc:
                padding-left: 10

                --- ftd.text: ref $toc.title
                link: ref $toc.link

                --- toc-item:
                $loop$: $toc.children as $obj
                toc: $obj


                -- toc-record list toc:
                $processor$: ft.toc

                - fifthtry/ftd/p1
                  `ftd::p1`: A JSON/YML Replacement
                - fifthtry/ftd/language
                  FTD Language
                  - fifthtry/ftd/p1-grammar
                    `ftd::p1` grammar




                -- toc-item:
                $loop$: $toc as $obj
                toc: $obj
                "
            ),
            &ftd::p2::TestLibrary {},
        )
        .expect("found error");
        // pretty_assertions::assert_eq!(g_bag, bag);
        // pretty_assertions::assert_eq!(g_col, main);
        // --- toc-item:
        //                 $loop$: $toc.children as $t
        //                 toc: $t
    }*/
}
