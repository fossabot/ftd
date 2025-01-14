#![allow(dead_code)]

#[derive(Debug, Clone, Default, PartialEq)]
pub struct InterpreterState {
    pub id: String,
    pub bag: ftd::Map<ftd::interpreter2::Thing>,
    pub to_process: Vec<(String, Vec<ftd::ast::AST>)>,
    pub parsed_libs: ftd::Map<ParsedDocument>,
    pub instructions: Vec<ftd::interpreter2::Component>,
}

impl InterpreterState {
    fn new(id: String) -> InterpreterState {
        InterpreterState {
            id,
            bag: ftd::interpreter2::default::default_bag(),
            ..Default::default()
        }
    }

    pub fn continue_(mut self) -> ftd::interpreter2::Result<Interpreter> {
        if let Some((id, ast_to_process)) = self.to_process.last() {
            let parsed_document = self.parsed_libs.get(id).unwrap();
            let name = parsed_document.name.to_string();
            let aliases = parsed_document.doc_aliases.clone();
            if let Some(ast) = ast_to_process.first() {
                let ast = ast.clone();
                let state = &mut self;

                let doc = ftd::interpreter2::TDoc::new_state(&name, &aliases, state);

                if ast.is_record() {
                    match ftd::interpreter2::Record::from_ast(ast, &doc)? {
                        ftd::interpreter2::StateWithThing::State(s) => return Ok(s),
                        ftd::interpreter2::StateWithThing::Thing(record) => {
                            self.bag.insert(
                                record.name.to_string(),
                                ftd::interpreter2::Thing::Record(record),
                            );
                        }
                    }
                } else if ast.is_or_type() {
                    match ftd::interpreter2::OrType::from_ast(ast, &doc)? {
                        ftd::interpreter2::StateWithThing::State(s) => return Ok(s),
                        ftd::interpreter2::StateWithThing::Thing(or_type) => {
                            self.bag.insert(
                                or_type.name.to_string(),
                                ftd::interpreter2::Thing::OrType(or_type),
                            );
                        }
                    }
                } else if ast.is_function() {
                    match ftd::interpreter2::Function::from_ast(ast, &doc)? {
                        ftd::interpreter2::StateWithThing::State(s) => return Ok(s),
                        ftd::interpreter2::StateWithThing::Thing(function) => {
                            self.bag.insert(
                                function.name.to_string(),
                                ftd::interpreter2::Thing::Function(function),
                            );
                        }
                    }
                } else if ast.is_variable_definition() {
                    match ftd::interpreter2::Variable::from_ast(ast, &doc)? {
                        ftd::interpreter2::StateWithThing::State(s) => return Ok(s),
                        ftd::interpreter2::StateWithThing::Thing(variable) => {
                            self.bag.insert(
                                variable.name.to_string(),
                                ftd::interpreter2::Thing::Variable(variable),
                            );
                        }
                    }
                } else if ast.is_variable_invocation() {
                    match ftd::interpreter2::Variable::update_from_ast(ast, &doc)? {
                        ftd::interpreter2::StateWithThing::State(s) => return Ok(s),
                        ftd::interpreter2::StateWithThing::Thing(variable) => {
                            self.bag.insert(
                                variable.name.to_string(),
                                ftd::interpreter2::Thing::Variable(variable),
                            );
                        }
                    }
                } else if ast.is_component_definition() {
                    match ftd::interpreter2::ComponentDefinition::from_ast(ast, &doc)? {
                        ftd::interpreter2::StateWithThing::State(s) => return Ok(s),
                        ftd::interpreter2::StateWithThing::Thing(component) => {
                            self.bag.insert(
                                component.name.to_string(),
                                ftd::interpreter2::Thing::Component(component),
                            );
                        }
                    }
                } else if ast.is_component() {
                    match ftd::interpreter2::Component::from_ast(ast, &doc)? {
                        ftd::interpreter2::StateWithThing::State(s) => return Ok(s),
                        ftd::interpreter2::StateWithThing::Thing(component) => {
                            self.instructions.push(component);
                        }
                    }
                }
                self.remove_last();
            }
        }

        if self
            .to_process
            .last()
            .map(|v| v.1.is_empty())
            .unwrap_or(false)
        {
            self.to_process.pop();
        }

        if self.to_process.is_empty() {
            let document = Document {
                data: self.bag,
                aliases: self
                    .parsed_libs
                    .get(self.id.as_str())
                    .unwrap()
                    .doc_aliases
                    .clone(),
                tree: self.instructions,
                name: self.id,
            };

            Ok(Interpreter::Done { document })
        } else {
            self.continue_()
        }
    }

    pub fn remove_last(&mut self) {
        let mut pop_last = false;
        if let Some((_, asts)) = self.to_process.last_mut() {
            if !asts.is_empty() {
                asts.remove(0);
            }
            if asts.is_empty() {
                pop_last = true;
            }
        }
        if pop_last {
            self.to_process.pop();
        }
    }

    pub fn continue_after_import(
        mut self,
        id: &str,
        source: &str,
        foreign_variable: Vec<String>,
        foreign_function: Vec<String>,
    ) -> ftd::interpreter2::Result<Interpreter> {
        let mut document = ParsedDocument::parse(id, source)?;
        document.add_foreign_function(foreign_function);
        document.add_foreign_variable(foreign_variable);
        self.parsed_libs.insert(id.to_string(), document);
        self.continue_()
    }

    pub fn continue_after_processor(
        mut self,
        value: ftd::interpreter2::Value,
    ) -> ftd::interpreter2::Result<Interpreter> {
        let (id, ast_to_process) = self.to_process.last().unwrap(); //TODO: remove unwrap & throw error
        let parsed_document = self.parsed_libs.get(id).unwrap();
        let name = parsed_document.name.to_string();
        let aliases = parsed_document.doc_aliases.clone();
        let ast = ast_to_process.first().unwrap().clone(); // TODO: remove unwrap
        let doc = ftd::interpreter2::TDoc::new_state(&name, &aliases, &mut self);
        let variable_definition = ast.get_variable_definition(doc.name)?;
        let name = doc.resolve_name(variable_definition.name.as_str());
        let kind = match ftd::interpreter2::KindData::from_ast_kind(
            variable_definition.kind,
            &Default::default(),
            &doc,
            variable_definition.line_number,
        )? {
            StateWithThing::Thing(t) => t,
            StateWithThing::State(s) => return Ok(s),
        };

        let value =
            value.into_property_value(variable_definition.mutable, variable_definition.line_number);

        let variable = ftd::interpreter2::Variable {
            name,
            kind,
            mutable: variable_definition.mutable,
            value,
            conditional_value: vec![],
            line_number: variable_definition.line_number,
            is_static: true,
        }
        .set_static(&doc);
        ftd::interpreter2::utils::validate_variable(&variable, &doc)?;
        self.bag.insert(
            variable.name.to_string(),
            ftd::interpreter2::Thing::Variable(variable),
        );
        self.remove_last();
        self.continue_()
    }

    pub fn continue_after_variable(
        mut self,
        module: &str,
        variable: &str,
        value: ftd::interpreter2::Value,
    ) -> ftd::interpreter2::Result<Interpreter> {
        let parsed_document = self.parsed_libs.get(module).unwrap();
        let name = parsed_document.name.to_string();
        let aliases = parsed_document.doc_aliases.clone();
        let doc = ftd::interpreter2::TDoc::new_state(&name, &aliases, &mut self);
        let var_name = doc.resolve_name(variable);
        let variable = ftd::interpreter2::Variable {
            name: var_name,
            kind: value.kind().into_kind_data(),
            mutable: false,
            value: value.into_property_value(false, 0),
            conditional_value: vec![],
            line_number: 0,
            is_static: true,
        }
        .set_static(&doc);
        ftd::interpreter2::utils::validate_variable(&variable, &doc)?;
        self.bag.insert(
            variable.name.to_string(),
            ftd::interpreter2::Thing::Variable(variable),
        );
        self.continue_()
    }
}

pub fn interpret<'a>(id: &'a str, source: &'a str) -> ftd::interpreter2::Result<Interpreter> {
    use itertools::Itertools;

    let mut s = InterpreterState::new(id.to_string());
    s.parsed_libs
        .insert(id.to_string(), ParsedDocument::parse(id, source)?);
    s.to_process.push((
        id.to_string(),
        s.parsed_libs
            .get(id)
            .unwrap()
            .ast
            .iter()
            .filter_map(|v| {
                if v.is_component() {
                    Some(v.to_owned())
                } else {
                    None
                }
            })
            .collect_vec(),
    ));

    s.continue_()
}

#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ParsedDocument {
    pub name: String,
    pub ast: Vec<ftd::ast::AST>,
    pub processing_imports: bool,
    pub doc_aliases: ftd::Map<String>,
    pub foreign_variable: Vec<String>,
    pub foreign_function: Vec<String>,
    pub instructions: Vec<ftd::interpreter2::Component>,
}

impl ParsedDocument {
    fn parse(id: &str, source: &str) -> ftd::interpreter2::Result<ParsedDocument> {
        let ast = ftd::ast::AST::from_sections(ftd::p11::parse(source, id)?.as_slice(), id)?;
        let doc_aliases = {
            let mut doc_aliases = ftd::interpreter2::default::default_aliases();
            for ast in ast.iter().filter(|v| v.is_import()) {
                if let ftd::ast::AST::Import(ftd::ast::Import { module, alias, .. }) = ast {
                    doc_aliases.insert(alias.to_string(), module.to_string());
                }
            }
            doc_aliases
        };
        Ok(ParsedDocument {
            name: id.to_string(),
            ast,
            processing_imports: true,
            doc_aliases,
            foreign_variable: vec![],
            foreign_function: vec![],
            instructions: vec![],
        })
    }

    fn done_processing_imports(&mut self) {
        self.processing_imports = false;
    }

    fn reorder(
        &mut self,
        _bag: &ftd::Map<ftd::interpreter2::Thing>,
    ) -> ftd::interpreter2::Result<()> {
        // TODO: reorder
        self.ast.reverse();
        Ok(())
    }

    pub fn get_doc_aliases(&self) -> ftd::Map<String> {
        self.doc_aliases.clone()
    }

    pub fn add_foreign_variable(&mut self, foreign_variable: Vec<String>) {
        self.foreign_variable.extend(foreign_variable);
    }

    pub fn add_foreign_function(&mut self, foreign_function: Vec<String>) {
        self.foreign_function.extend(foreign_function);
    }
}

#[derive(Debug)]
pub enum Interpreter {
    StuckOnImport {
        module: String,
        state: InterpreterState,
    },
    Done {
        document: Document,
    },
    StuckOnProcessor {
        state: InterpreterState,
        ast: ftd::ast::AST,
        module: String,
    },
    StuckOnForeignVariable {
        state: InterpreterState,
        module: String,
        variable: String,
    },
}

#[derive(Debug, Default, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Document {
    pub data: ftd::Map<ftd::interpreter2::Thing>,
    pub name: String,
    pub tree: Vec<ftd::interpreter2::Component>,
    pub aliases: ftd::Map<String>,
}

#[derive(Debug)]
pub enum StateWithThing<T> {
    Thing(T),
    State(Interpreter),
}

impl<T> StateWithThing<T> {
    pub fn new_thing(thing: T) -> StateWithThing<T> {
        StateWithThing::Thing(thing)
    }

    pub fn new_state(state: Interpreter) -> StateWithThing<T> {
        StateWithThing::State(state)
    }

    pub fn map<U, F: FnOnce(T) -> U>(self, f: F) -> StateWithThing<U> {
        let thing = try_state!(self);
        StateWithThing::new_thing(f(thing))
    }
}
