use lsp_types::{Diagnostic, Position, Range};

use analysis::analysis::Analysis;
use analysis::change::AnalysisChange;
use analysis::config::Config;
use analysis::db::FileDiagnostic;
use integration_tests::{get_modules_path, get_test_resources_dir, global_state_snapshot};
use move_language_server::global_state::initialize_new_global_state;

use crossbeam_channel::unbounded;
use move_language_server::main_loop::{compute_file_diagnostics, Task};
use utils::{leaked_fpath, MoveFile, MoveFilePath};

fn range(start: (u64, u64), end: (u64, u64)) -> Range {
    Range::new(Position::new(start.0, start.1), Position::new(end.0, end.1))
}

fn diagnostics(text: &str) -> Vec<Diagnostic> {
    diagnostics_with_config(text, Config::default())
}

fn diagnostics_with_config(text: &str, config: Config) -> Vec<Diagnostic> {
    let loc_ds = diagnostics_with_config_and_filename(
        text,
        config,
        leaked_fpath(get_test_resources_dir().join("some_script.move")),
    );
    loc_ds.into_iter().filter_map(|d| d.diagnostic).collect()
}

fn diagnostics_with_config_and_filename(
    text: &str,
    config: Config,
    fpath: MoveFilePath,
) -> Vec<FileDiagnostic> {
    let state_snapshot = global_state_snapshot((fpath, text.to_string()), config, vec![]);
    let (task_sender, task_receiver) = unbounded::<Task>();

    compute_file_diagnostics(state_snapshot.analysis, task_sender, vec![fpath]);

    let task = task_receiver.try_recv().unwrap();
    let mut ds = match task {
        Task::Diagnostic(ds) => ds,
        _ => panic!(),
    };
    let empty = ds.remove(0);
    assert!(empty.diagnostic.is_none());
    ds
}

fn diagnostics_with_deps(
    script_file: MoveFile,
    deps: Vec<MoveFile>,
    config: Config,
) -> Option<FileDiagnostic> {
    let (script_fpath, script_text) = script_file;
    let mut config = config;
    config.update(&serde_json::json!({
        "modules_folders": [get_modules_path()]
    }));

    let ws_root = std::env::current_dir().unwrap();
    let global_state = initialize_new_global_state(ws_root, config);
    let mut analysis_host = global_state.analysis_host;

    let mut change = AnalysisChange::new();
    for (fpath, text) in deps.into_iter() {
        change.add_file(fpath, text);
    }
    change.add_file(script_fpath, script_text.clone());
    analysis_host.apply_change(change);

    analysis_host
        .analysis()
        .check_file_with_compiler(script_fpath, &script_text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use analysis::db::RootDatabase;

    use integration_tests::{
        config, get_modules_path, get_script_path, get_stdlib_path, modules_mod,
    };
    use utils::{leaked_fpath, FilesSourceText};

    #[test]
    fn test_fail_on_non_ascii_character() {
        let source_text = r"fun main() { return; }ффф";
        let errors = diagnostics(source_text);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].range, range((0, 22), (0, 22)));
    }

    #[test]
    fn test_successful_compilation() {
        let source = r"
script {
    fun main() {}
}
    ";
        let errors = diagnostics(source);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_function_parse_error() {
        let source_text = "module M { struc S { f: u64 } }";
        let errors = diagnostics(source_text);
        assert_eq!(errors.len(), 1);

        assert_eq!(errors[0].message, "Unexpected 'struc'");
        assert_eq!(errors[0].range, range((0, 11), (0, 16)));
    }

    #[test]
    fn test_main_function_parse_error() {
        let source_text = "script { main() {} }";
        let errors = diagnostics(source_text);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "Unexpected 'main'");
    }

    #[test]
    fn test_multiline_function_parse_error() {
        let source_text = r"
module M {
    struc S {
        f: u64
    }
}
";
        let errors = diagnostics(source_text);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "Unexpected \'struc\'");
    }

    #[test]
    fn test_expansion_checks_duplicates() {
        let source_text = r"
module M {
    struct S {
        f: u64,
        f: u64,
    }
}
    ";
        let errors = diagnostics(source_text);
        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors[0].message,
            "Duplicate definition for field \'f\' in struct \'S\'"
        );
    }

    #[test]
    fn test_expansion_checks_public_main_redundancy() {
        let source_text = r"script { public fun main() {} }";

        let errors = diagnostics(source_text);
        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors[0].message,
            "Extraneous 'public' modifier. Script functions are always public"
        );
    }

    #[test]
    fn test_naming_checks_generics_with_type_parameters() {
        let source_text = r"
module M {
    struct S<T> { f: T<u64> }
}
    ";

        let errors = diagnostics(source_text);
        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors[0].message,
            "Generic type parameters cannot take type arguments"
        );
    }

    #[test]
    fn test_typechecking_invalid_local_borrowing() {
        let source_text = r"
module M {
    fun t0(r: &u64) {
        &r;
    }
}
    ";
        let errors = diagnostics(source_text);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "Invalid borrow");
    }

    #[test]
    fn test_stdlib_modules_are_available_if_loaded() {
        let source_text = r"
module MyModule {
    use 0x1::Signer;

    public fun how_main(s: &signer) {
        let _ = Signer::address_of(s);
    }
}
    ";
        let errors =
            diagnostics_with_config(source_text, config!({ "stdlib_folder": get_stdlib_path() }));
        assert!(errors.is_empty());
    }

    #[test]
    fn test_compile_check_script_with_additional_dependencies() {
        // hardcoded sender address
        let script_source_text = r"
script {
    use 0x1::Signer;
    use 0x2::Record;

    fun main(s: &signer) {
        let signer_address = Signer::address_of(s);
        let record = Record::get_record(signer_address);
        Record::save(s, record);
    }
}
    ";
        let config = config!({
            "dialect": "libra",
            "sender_address": "0x8572f83cee01047effd6e7d0b5c19743",
            "stdlib_folder": get_stdlib_path(),
            "modules_folders": [get_modules_path()],
        });
        let errors = diagnostics_with_config(script_source_text, config);
        assert!(errors.is_empty(), "{:#?}", errors);
    }

    #[test]
    fn test_compile_check_module_from_a_folder_with_folder_provided_as_dependencies() {
        let (record_module_fpath, record_module_text) = modules_mod("record.move");
        let config = config!({
            "stdlib_folder": get_stdlib_path(),
            "modules_folders": [get_modules_path()],
        });

        let errors = diagnostics_with_config_and_filename(
            &record_module_text,
            config,
            record_module_fpath,
        );
        assert!(errors.is_empty(), "{:#?}", errors);
    }

    #[test]
    fn test_compile_with_sender_address_specified() {
        // hardcoded sender address
        let script_source_text = r"
script {
    use 0x1::Signer;
    use 0x2::Record;

    fun main(s: &signer) {
        let signer_address = Signer::address_of(s);
        let record = Record::get_record(signer_address);
        Record::save(s, record);
    }
}
    ";
        let config = config!({
            "dialect": "libra",
            "stdlib_folder": get_stdlib_path(),
            "modules_folders": [get_modules_path()],
            "sender_address": "0x1",
        });
        let errors = diagnostics_with_config(script_source_text, config);
        assert!(errors.is_empty(), "{:#?}", errors);
    }

    #[test]
    fn test_compiler_out_of_bounds_multimessage_diagnostic() {
        let source_text = r"
script {
    use 0x1::Signer;
    use 0x2::Record;

    fun main(s: &signer) {
        let signer_address = Signer::address_of(s);
        let record: u8;
        record = Record::get_record(signer_address);
    }
}    ";
        let config = config!({
            "stdlib_folder": get_stdlib_path(),
            "modules_folders": [get_modules_path()]
        });
        let errors = diagnostics_with_config(source_text, config);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].related_information.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn test_syntax_error_in_dependency() {
        let config = config!({ "modules_folders": [get_modules_path()] });

        let mut files = FilesSourceText::new();
        let dep_module_fpath =
            leaked_fpath(get_modules_path().join("dep_module.move").to_str().unwrap());
        let dep_module_source_text = "address 0x0 { modules T { public fun how_many() {} } }";
        files.insert(dep_module_fpath, dep_module_source_text.to_string());

        let main_fpath = leaked_fpath(get_modules_path().join("module.move").to_str().unwrap());
        let source_text = r"
    module HowMany {
        use 0x0::T;
        public fun how() {
            T::how_many()
        }
    }
    ";
        files.insert(main_fpath, source_text.to_string());

        let db = RootDatabase {
            config,
            available_files: files,
        };
        let analysis = Analysis::new(db);
        let error = analysis
            .check_file_with_compiler(main_fpath, source_text)
            .unwrap();
        assert_eq!(error.fpath, dep_module_fpath);
        assert_eq!(
            error.diagnostic.as_ref().unwrap().message,
            "Unexpected 'modules'"
        );
    }

    #[test]
    fn test_check_one_of_the_stdlib_modules_no_duplicate_definition() {
        let source_text = r"
address 0x1 {
    module Debug {
        native public fun print<T>(x: &T);

        native public fun print_stack_trace();
    }
}
    ";
        let config = config!({
            "stdlib_folder": get_stdlib_path(),
        });
        let errors = diagnostics_with_config_and_filename(
            source_text,
            config,
            leaked_fpath(get_stdlib_path().join("debug.move")),
        );
        assert!(errors.is_empty(), "{:?}", errors);
    }

    #[test]
    fn invalid_valid_in_precense_of_bech32_address() {
        let invalid_source_text = r"
address 0x12345 {
    module Debug {
        pubic fun main() {}
    }
}
    ";
        let errors =
            diagnostics_with_config(invalid_source_text, config!({"dialect": "starcoin"}));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "Unexpected \'pubic\'");
        assert_eq!(errors[0].range, range((3, 8), (3, 13)))
    }

    #[test]
    fn two_bech32_addresses_one_in_the_middle_of_script() {
        let source_text = r"
address 0x12345 {
    module Debug {
        public fun main() {
            let _ = 0x12345;
        }
    }
}
    ";
        let errors = diagnostics_with_config(source_text, config!({"dialect": "starcoin"}));
        assert!(errors.is_empty(), "{:?}", errors);

        let invalid_source_text = r"
address 0x12345 {
    module Debug {
        public fun main() {
            let addr = 0x12345;
        }
    }
}
    ";
        let errors =
            diagnostics_with_config(invalid_source_text, config!({"dialect": "starcoin"}));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "Unused assignment or binding for local 'addr'. Consider removing or replacing it with '_'");
        assert_eq!(errors[0].range, range((4, 16), (4, 20)));

        let invalid_source_text = r"
address 0x12345 {
    module Debug {
        public fun main() {
            let _ = 0x12345;
            let _ = 0x12345;
            let _: u10;
        }
    }
}
    ";
        let errors =
            diagnostics_with_config(invalid_source_text, config!({"dialect": "starcoin"}));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "Unbound type 'u10' in current scope");
        assert_eq!(errors[0].range, range((6, 19), (6, 22)));
    }

    #[test]
    fn pass_bech32_address_as_sender() {
        let source_text = r"
address 0x12345 {
    module Debug {
        public fun main() {}
    }
}
    ";
        let config = config!({
            "dialect": "starcoin",
            "sender_address": "0x12345"
        });
        let errors = diagnostics_with_config(source_text, config);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    #[test]
    fn test_substitude_sender_as_template_syntax() {
        let source_text = r"
address {{sender}} {
    module Debug {
        public fun main() {
            let _ = {{sender}};
        }
    }
}";
        let config = config!({
            "dialect": "libra",
            "sender_address": "0x1111111111111111"
        });
        let errors = diagnostics_with_config(source_text, config);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    #[test]
    fn test_substitude_sender_as_template_syntax_with_spaces() {
        let source_text = r"
address {{ sender }} {
    module Debug {
        public fun main() {
            let _ = {{ sender }};
        }
    }
}";
        let config = config!({
            "dialect": "libra",
            "sender_address": "0x1111111111111111"
        });
        let errors = diagnostics_with_config(source_text, config);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    #[test]
    fn test_sender_substitution_with_errors() {
        let source_text = r"
address {{sender}} {
    module Debug {
        public fun debug() {
            let _ = 0x0::Unknown::unknown();
        }
    }
}";
        let config = config!({
            "dialect": "libra",
            "sender_address": "0x1111111111111111"
        });
        let errors = diagnostics_with_config(source_text, config);
        assert_eq!(errors[0].message, "Unbound module \'0x0::Unknown\'");
        assert_eq!(errors[0].range, range((4, 20), (4, 41)));
    }

    #[test]
    fn test_multiple_substitutions_with_sender() {
        let source_text = r"
address {{sender}} {
    module Debug {
        public fun debug() {
            let _ = {{sender}};
            let _ = 0x0::Unknown::unknown();
        }
    }
}";
        let config = config!({
            "dialect": "libra",
            "sender_address": "0x1111111111111111"
        });
        let errors = diagnostics_with_config(source_text, config);
        assert_eq!(errors[0].message, "Unbound module \'0x0::Unknown\'");
        assert_eq!(errors[0].range, range((5, 20), (5, 41)));
    }

    #[test]
    fn test_bech32_and_sender_substitution_with_errors() {
        let source_text = r"
address {{ sender }} {
    module Debug {
        public fun main() {
            let _ = 0x12345;
            let _ = {{ sender }};
            // errors out
            0x0::Unknown::unknown();
        }
    }
}";
        let config = config!({
            "dialect": "starcoin",
            "sender_address": "0x12345"
        });
        let errors = diagnostics_with_config(source_text, config);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "Unbound module \'0x0::Unknown\'");
        assert_eq!(errors[0].range, range((7, 12), (7, 33)));
    }

    #[test]
    fn test_replace_with_longer_form_if_sender_shorter_than_template_string() {
        let source_text = r"
address {{sender}} {
    module Debug {
        public fun main() {}
    }
}";
        let config = config!({
            "dialect": "libra",
            "sender_address": "0x1"
        });
        let errors = diagnostics_with_config(source_text, config);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    #[test]
    fn test_sender_replacement_in_script() {
        let module_text = r"
address {{sender}} {
    module Debug {
        public fun debug(): u8 {
            1
        }
    }
}";
        let source_text = r"
script {
    fun main() {
        let _ = {{sender}}::Debug::debug();
    }
}
        ";
        let config = config!({
            "dialect": "libra",
            "sender_address": "0x1",
        });
        let error = diagnostics_with_deps(
            (get_script_path(), source_text.to_string()),
            vec![(
                leaked_fpath(get_modules_path().join("debug.move")),
                module_text.to_string(),
            )],
            config,
        );
        assert!(error.is_none(), "{:#?}", error);
    }

    #[test]
    fn test_error_message_for_unbound_module_with_bech32_address() {
        let text = r"
script {
    fun main() {
        let _ = 0x12345::Unknown::unknown();
    }
}
        ";
        let config = config!({"dialect": "starcoin"});
        let errors = diagnostics_with_config(text, config);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "Unbound module \'0x12345::Unknown\'")
    }

    #[test]
    fn test_error_message_unbound_module_with_bech32_address_and_sender() {
        let text = r"
script {
    fun main() {
        let _ = {{sender}}::Unknown::unknown();
    }
}
        ";
        let config = config!({
            "dialect": "starcoin",
            "sender_address": "0x12345"
        });
        let errors = diagnostics_with_config(text, config);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].range, range((3, 16), (3, 44)));
        assert_eq!(errors[0].message, "Unbound module \'0x12345::Unknown\'")
    }

    #[test]
    fn test_starcoin_documentation_issue_should_not_crash_with_span_overflow() {
        let dfi_module_text = r"
address 0x0 {
/// docs
module DFI {
    struct T {}
}
}";
        let errors = diagnostics(dfi_module_text);
        assert_eq!(errors.len(), 0);
    }

    #[test]
    fn test_when_module_resolution_fails_error_should_be_at_use_site() {
        let script_text = r"script {
            use 0x0::UnknownPayments;
            fun main(s: &signer) {
                UnknownPayments::send_payment_event();
            }
        }";
        let errors = diagnostics(script_text);
        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors[0].message,
            "Invalid \'use\'. Unbound module: \'0x0::UnknownPayments\'"
        );
    }
}
