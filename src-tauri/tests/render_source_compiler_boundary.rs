use ecky_cad_lib::contracts::{DesignParams, GeometryBackend};
use ecky_cad_lib::ecky_cad_host::direct_occt::plan_core_program;
use ecky_cad_lib::ecky_cad_host::source_compiler::NativeSourceCompiler;
use ecky_cad_lib::models::PathResolver;
use ecky_cad_lib::services::render::render_cli_ecky;
use ecky_render::core_ir::{CoreNodeKind, CoreOperation, CorePrimitive};
use ecky_render::SourceCompiler;
use std::path::PathBuf;

struct RenderPaths(PathBuf);

impl PathResolver for RenderPaths {
    fn app_config_dir(&self) -> PathBuf {
        self.0.join("config")
    }

    fn app_data_dir(&self) -> PathBuf {
        self.0.join("data")
    }

    fn resource_path(&self, _path: &str) -> Option<PathBuf> {
        None
    }
}

#[test]
fn native_source_compiler_implements_the_render_crate_port() {
    let program = NativeSourceCompiler
        .compile("(model (part body (box 10 20 30)))")
        .expect("source compiles through port");

    assert_eq!(program.parts.len(), 1);
    assert!(matches!(
        program.parts[0].root.kind,
        CoreNodeKind::Call {
            op: CoreOperation::Primitive(CorePrimitive::Box),
            ..
        }
    ));
}

#[test]
fn text_renders_with_futura_collection() {
    let components = ecky_cad_lib::ecky_cad_host::text_profile::parse_text_profile(
        "Понедельник",
        10.0,
        Some("/System/Library/Fonts/Supplemental/Futura.ttc"),
    )
    .expect("Futura Cyrillic text profile compiles by path");
    assert!(!components.is_empty());

    let family_components = ecky_cad_lib::ecky_cad_host::text_profile::parse_text_profile(
        "Понедельник",
        10.0,
        Some("Futura"),
    )
    .expect("Futura Cyrillic text profile compiles by family name");
    assert!(!family_components.is_empty());
}

#[test]
fn single_contour_curved_cyrillic_text_reaches_direct_occt_as_a_closed_profile() {
    let font = "/System/Library/Fonts/Supplemental/Arial.ttf";
    if !std::path::Path::new(font).is_file() {
        return;
    }
    let source = format!("(model (part letter (extrude (text \"С\" 12 :font \"{font}\") 2)))");
    let program = NativeSourceCompiler
        .compile(&source)
        .expect("Cyrillic text source compiles");

    plan_core_program(&program).expect("curved glyph lowers to a valid direct OCCT profile");

    let root = std::env::temp_dir().join(format!("ecky-curved-text-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("data")).expect("test data directory");
    std::fs::create_dir_all(root.join("config")).expect("test config directory");
    let rendered = render_cli_ecky(
        &source,
        &DesignParams::new(),
        GeometryBackend::EckyRust,
        None,
        &RenderPaths(root.clone()),
    )
    .expect("curved Cyrillic glyph renders through direct OCCT");
    assert!(std::path::Path::new(&rendered.model_stl_path).is_file());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn cyrillic_pair_lookup_renders_from_a_select_parameter() {
    let font = "/System/Library/Fonts/Supplemental/Arial.ttf";
    if !std::path::Path::new(font).is_file() {
        return;
    }
    let source = format!(
        r#"(define (pair-for upper)
              (cadr (assoc upper '(("А" "Аа") ("Б" "Бб") ("В" "Вв") ("Г" "Гг")
                                    ("Д" "Дд") ("Е" "Ее") ("Ё" "Ёё") ("Ж" "Жж")
                                    ("З" "Зз") ("И" "Ии") ("Й" "Йй") ("К" "Кк")
                                    ("Л" "Лл") ("М" "Мм") ("Н" "Нн") ("О" "Оо")
                                    ("П" "Пп") ("Р" "Рр") ("С" "Сс") ("Т" "Тт")
                                    ("У" "Уу") ("Ф" "Фф") ("Х" "Хх") ("Ц" "Цц")
                                    ("Ч" "Чч") ("Ш" "Шш") ("Щ" "Щщ") ("Ъ" "Ъъ")
                                    ("Ы" "Ыы") ("Ь" "Ьь") ("Э" "Ээ") ("Ю" "Юю")
                                    ("Я" "Яя")))))
            (model
              (params (select letter "А" :options '(("А" "А") ("Г" "Г"))))
              (part card (extrude (text (pair-for letter) 12 :font "{font}") 2)))"#
    );
    let root = std::env::temp_dir().join(format!("ecky-pair-lookup-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("data")).expect("test data directory");
    std::fs::create_dir_all(root.join("config")).expect("test config directory");
    let params = DesignParams::from([(
        "letter".to_string(),
        ecky_cad_lib::contracts::ParamValue::String("Я".into()),
    )]);
    let rendered = render_cli_ecky(
        &source,
        &params,
        GeometryBackend::EckyRust,
        None,
        &RenderPaths(root.clone()),
    )
    .expect("selected Cyrillic pair renders through direct OCCT");
    assert!(std::path::Path::new(&rendered.model_stl_path).is_file());
    let _ = std::fs::remove_dir_all(root);
}
