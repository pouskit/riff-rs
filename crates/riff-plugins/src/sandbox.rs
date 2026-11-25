//! Moteur Rhai d'un plugin : paquets sans E/S, limites explicites, compilation.

use rhai::module_resolvers::DummyModuleResolver;
use rhai::packages::{
  ArithmeticPackage, BasicArrayPackage, BasicFnPackage, BasicIteratorPackage, BasicMapPackage,
  BasicMathPackage, BasicStringPackage, LogicPackage, MoreStringPackage, Package,
};
use rhai::{AST, Engine};

/// Opérations par appel. Le budget de temps (`on_progress`) arrête le script avant dans la plupart des cas.
pub const MAX_OPERATIONS: u64 = 20_000_000;
/// Profondeur d'appels de fonctions : la récursion infinie s'arrête là.
pub const MAX_CALL_LEVELS: usize = 32;
/// Profondeur d'expressions, au niveau global et dans les fonctions.
pub const MAX_EXPR_DEPTH: usize = 64;
pub const MAX_FUNCTION_EXPR_DEPTH: usize = 32;
/// Taille d'une chaîne, en octets.
pub const MAX_STRING_BYTES: usize = 256 * 1024;
/// Éléments d'un tableau, et propriétés d'une carte.
pub const MAX_ARRAY_LEN: usize = 10_000;
pub const MAX_MAP_LEN: usize = 10_000;
pub const MAX_VARIABLES: usize = 1_024;
pub const MAX_FUNCTIONS: usize = 512;

/// Erreur de syntaxe, avec sa position (ligne et colonne à partir de 1).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message} (ligne {line}, colonne {column})")]
pub struct CompileError {
  pub message: String,
  pub line: usize,
  pub column: usize,
}

/// Moteur nu : aucun paquet d'E/S, aucun module, pas d'`eval`, toutes les limites fixées.
/// Les fonctions de l'hôte sont ajoutées ensuite, selon les permissions.
#[must_use]
pub fn base_engine() -> Engine {
  let mut engine = Engine::new_raw();
  // `CorePackage` est évité : il contient `LanguageCorePackage` et son `sleep` natif, hors budget.
  ArithmeticPackage::new().register_into_engine(&mut engine);
  LogicPackage::new().register_into_engine(&mut engine);
  BasicMathPackage::new().register_into_engine(&mut engine);
  BasicStringPackage::new().register_into_engine(&mut engine);
  MoreStringPackage::new().register_into_engine(&mut engine);
  BasicIteratorPackage::new().register_into_engine(&mut engine);
  BasicFnPackage::new().register_into_engine(&mut engine);
  BasicArrayPackage::new().register_into_engine(&mut engine);
  BasicMapPackage::new().register_into_engine(&mut engine);
  engine
    .set_module_resolver(DummyModuleResolver::new())
    .set_max_modules(0)
    .set_max_operations(MAX_OPERATIONS)
    .set_max_call_levels(MAX_CALL_LEVELS)
    .set_max_expr_depths(MAX_EXPR_DEPTH, MAX_FUNCTION_EXPR_DEPTH)
    .set_max_string_size(MAX_STRING_BYTES)
    .set_max_array_size(MAX_ARRAY_LEN)
    .set_max_map_size(MAX_MAP_LEN)
    .set_max_variables(MAX_VARIABLES)
    .set_max_functions(MAX_FUNCTIONS)
    .disable_symbol("eval");
  engine
}

/// Compile une source avec le moteur donné.
///
/// # Errors
/// Erreur de syntaxe avec ligne et colonne.
pub fn compile_with(engine: &Engine, code: &str) -> Result<AST, CompileError> {
  engine.compile(code).map_err(|err| CompileError {
    message: err.err_type().to_string(),
    line: err.position().line().unwrap_or(0),
    column: err.position().position().unwrap_or(0),
  })
}

/// Compile une source avec le moteur nu : vérification de syntaxe sans exécution.
///
/// # Errors
/// Erreur de syntaxe avec ligne et colonne.
pub fn compile(code: &str) -> Result<AST, CompileError> {
  compile_with(&base_engine(), code)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_valid_plugin_compiles_and_exposes_its_functions() {
    let ast =
      compile("// @name A\nfn on_ready() { 1 + 1 }\nfn on_trackchange(track) { track.title }\n")
        .unwrap();
    let names: Vec<String> = ast.iter_functions().map(|f| f.name.to_string()).collect();
    assert!(names.contains(&"on_ready".to_string()));
    assert!(names.contains(&"on_trackchange".to_string()));
  }

  #[test]
  fn a_syntax_error_gives_its_line_and_column() {
    let err = compile("// @name A\nfn on_ready() {\n  let x = ;\n}\n").unwrap_err();
    assert_eq!(err.line, 3);
    assert!(err.column > 0, "{err}");
    assert!(err.to_string().contains("ligne 3"));
  }

  #[test]
  fn eval_is_not_a_keyword_any_more() {
    let err = compile("fn on_ready() { eval(\"1\") }").unwrap_err();
    assert!(err.line >= 1, "{err}");
  }

  #[test]
  fn an_import_cannot_resolve_a_file() {
    let engine = base_engine();
    let err = engine.run("import \"secret\" as s;").unwrap_err();
    assert!(
      matches!(
        *err,
        rhai::EvalAltResult::ErrorModuleNotFound(..) | rhai::EvalAltResult::ErrorTooManyModules(..)
      ),
      "{err}"
    );
  }

  #[test]
  fn sleep_and_time_functions_do_not_exist() {
    let engine = base_engine();
    for code in ["sleep(10)", "exit()", "timestamp()", "parse_json(\"{}\")"] {
      let err = engine.run(code).unwrap_err();
      assert!(
        matches!(*err, rhai::EvalAltResult::ErrorFunctionNotFound(..)),
        "{code}: {err}"
      );
    }
  }

  #[test]
  fn print_and_debug_write_nothing_and_do_not_fail() {
    base_engine().run("print(1); debug(2);").unwrap();
  }

  #[test]
  fn basic_language_features_work_without_the_core_package() {
    let engine = base_engine();
    let value: i64 = engine
      .eval("let a = [3, 1, 2]; a.sort(); let m = #{ x: a.len() }; m.x + a[0] + \"ab\".len()")
      .unwrap();
    assert_eq!(value, 3 + 1 + 2);
    let floor: f64 = engine.eval("(0.75 * 4.0).floor()").unwrap();
    assert!((floor - 3.0).abs() < f64::EPSILON);
  }
}
