use lsp_types::CompletionItem;

use crate::change::AnalysisChange;
use crate::completion;
use crate::db::{FileDiagnostic, FilePosition, RootDatabase};
use utils::{io, MoveFilePath};

#[derive(Debug, Default)]
pub struct AnalysisHost {
    db: RootDatabase,
}

impl AnalysisHost {
    pub fn db(&self) -> &RootDatabase {
        &self.db
    }

    pub fn analysis(&self) -> Analysis {
        Analysis::new(self.db.clone())
    }

    pub fn apply_change(&mut self, change: AnalysisChange) {
        self.db.apply_change(change);
    }
}

#[derive(Debug)]
pub struct Analysis {
    db: RootDatabase,
}

impl Analysis {
    pub fn new(db: RootDatabase) -> Analysis {
        Analysis { db }
    }

    pub fn db(&self) -> &RootDatabase {
        &self.db
    }

    pub fn completions(&self, position: FilePosition) -> Vec<CompletionItem> {
        completion::completions(self.db(), position)
    }

    pub fn check_file_with_compiler(
        &self,
        fpath: MoveFilePath,
        text: &str,
    ) -> Option<FileDiagnostic> {
        match self.check_file_with_compiler_inner(fpath, text) {
            Ok(_) => None,
            Err(mut ds) => Some(ds.remove(0)),
        }
    }

    #[inline]
    fn check_file_with_compiler_inner(
        &self,
        current_fpath: MoveFilePath,
        current_text: &str,
    ) -> Result<(), Vec<FileDiagnostic>> {
        let deps: Vec<(MoveFilePath, String)> = self
            .read_stdlib_files()
            .into_iter()
            .chain(self.db.module_files().into_iter())
            .filter(|(fpath, _)| *fpath != current_fpath)
            .collect();

        let current_file = (current_fpath, current_text.to_string());
        self.db
            .config
            .dialect()
            .check_with_compiler(current_file, deps, self.db.config.sender())
            .map_err(|errors| {
                errors
                    .into_iter()
                    .map(
                        |err| match self.db.compiler_error_into_diagnostic(err.clone()) {
                            Ok(d) => d,
                            Err(error) => panic!(
                                "While converting {:#?} into Diagnostic, error occurred: {:?}",
                                err,
                                error.to_string()
                            ),
                        },
                    )
                    .collect()
            })
    }

    fn read_stdlib_files(&self) -> Vec<(MoveFilePath, String)> {
        self.db
            .config
            .stdlib_folder
            .as_ref()
            .map(|folder| io::read_move_files(folder.as_path()))
            .unwrap_or_else(Vec::new)
    }
}
