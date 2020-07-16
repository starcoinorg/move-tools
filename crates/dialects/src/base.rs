use anyhow::Result;
use move_core_types::gas_schedule::CostTable;
use move_lang::parser::ast::Definition;
use move_lang::parser::syntax;
use move_lang::strip_comments_and_verify;
use utils::MoveFile;
use vm::file_format::CompiledScript;
use vm::CompiledModule;

use crate::lang::executor::generate_bytecode;
use crate::lang::{check_defs, into_exec_compiler_error, replace_sender_placeholder};
use crate::shared::errors::{CompilerError, ExecCompilerError, FileSourceMap, ProjectSourceMap};

use crate::shared::ProvidedAccountAddress;

pub trait Dialect {
    fn name(&self) -> &str;

    fn normalize_account_address(&self, addr: &str) -> Result<ProvidedAccountAddress>;

    fn replace_addresses(&self, source_text: &str, source_map: &mut FileSourceMap) -> String;

    fn parse_file(
        &self,
        file: MoveFile,
        sender: &ProvidedAccountAddress,
    ) -> Result<(Vec<Definition>, FileSourceMap), ExecCompilerError> {
        let (fname, mut source_text) = file;

        let mut file_source_map = FileSourceMap::default();
        source_text = replace_sender_placeholder(
            source_text,
            &sender.normalized_original,
            &mut file_source_map,
        );
        source_text = self.replace_addresses(&source_text, &mut file_source_map);

        let (source_text, comment_map) =
            strip_comments_and_verify(fname, &source_text).map_err(|errors| {
                into_exec_compiler_error(
                    errors,
                    ProjectSourceMap::with_file_map(fname, FileSourceMap::default()),
                )
            })?;

        let (defs, _) =
            syntax::parse_file_string(fname, &source_text, comment_map).map_err(|errors| {
                into_exec_compiler_error(
                    errors,
                    ProjectSourceMap::with_file_map(fname, file_source_map.clone()),
                )
            })?;
        Ok((defs, file_source_map))
    }

    fn parse_files(
        &self,
        current_file: MoveFile,
        deps: &[MoveFile],
        sender: &ProvidedAccountAddress,
    ) -> Result<(Vec<Definition>, Vec<Definition>, ProjectSourceMap), ExecCompilerError> {
        let mut exec_compiler_error = ExecCompilerError::default();

        let mut project_offsets_map = ProjectSourceMap::default();
        let script_defs = match self.parse_file(current_file.clone(), &sender) {
            Ok((defs, offsets_map)) => {
                project_offsets_map.0.insert(current_file.0, offsets_map);
                defs
            }
            Err(error) => {
                exec_compiler_error.extend(error);
                vec![]
            }
        };

        let mut dep_defs = vec![];
        for dep_file in deps.iter() {
            let defs = match self.parse_file(dep_file.clone(), &sender) {
                Ok((defs, offsets_map)) => {
                    project_offsets_map.0.insert(dep_file.0, offsets_map);
                    defs
                }
                Err(error) => {
                    exec_compiler_error.extend(error);
                    vec![]
                }
            };
            dep_defs.extend(defs);
        }
        if !exec_compiler_error.0.is_empty() {
            return Err(exec_compiler_error);
        }
        Ok((script_defs, dep_defs, project_offsets_map))
    }

    fn check_with_compiler(
        &self,
        current: MoveFile,
        deps: Vec<MoveFile>,
        sender: &ProvidedAccountAddress,
    ) -> Result<(), Vec<CompilerError>> {
        let (script_defs, dep_defs, offsets_map) = self
            .parse_files(current, &deps, sender)
            .map_err(|errors| errors.transform_with_source_map())?;

        match check_defs(script_defs, dep_defs, sender.as_address()) {
            Ok(_) => Ok(()),
            Err(errors) => {
                Err(into_exec_compiler_error(errors, offsets_map).transform_with_source_map())
            }
        }
    }

    fn check_and_generate_bytecode(
        &self,
        file: MoveFile,
        deps: &[MoveFile],
        sender: ProvidedAccountAddress,
    ) -> Result<(CompiledScript, Vec<CompiledModule>), ExecCompilerError> {
        let (mut script_defs, modules_defs, project_offsets_map) =
            self.parse_files(file, deps, &sender)?;
        script_defs.extend(modules_defs);

        let program = check_defs(script_defs, vec![], sender.as_address())
            .map_err(|errors| into_exec_compiler_error(errors, project_offsets_map.clone()))?;
        generate_bytecode(program)
            .map_err(|errors| into_exec_compiler_error(errors, project_offsets_map))
    }
}
