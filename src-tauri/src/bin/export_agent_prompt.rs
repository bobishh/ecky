use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use ecky_cad_lib::agent_prompt::agent_language_reference;
use ecky_cad_lib::contracts::GeometryBackend;
use ecky_cad_lib::ecky_language_surface::supported_surface_reference;

const OP_INDEX_START: &str = "<!-- ECKY_GENERATED_OP_INDEX_START -->";
const OP_INDEX_END: &str = "<!-- ECKY_GENERATED_OP_INDEX_END -->";
const SURFACE_REFERENCE_START: &str = "<!-- ECKY_GENERATED_SURFACE_REFERENCE_START -->";
const SURFACE_REFERENCE_END: &str = "<!-- ECKY_GENERATED_SURFACE_REFERENCE_END -->";

fn main() {
    let output_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs/generated");
    fs::create_dir_all(&output_dir).expect("create generated docs directory");

    for (name, backend) in [
        ("ecky-rust", GeometryBackend::EckyRust),
        ("freecad", GeometryBackend::Freecad),
    ] {
        write_prompt(
            &output_dir.join(format!("ecky-agent-system-prompt-{name}.md")),
            backend,
        );
    }

    write_prompt(
        &output_dir.join("ecky-agent-system-prompt.md"),
        GeometryBackend::EckyRust,
    );
    write_book_operation_index();
}

fn write_prompt(path: &Path, backend: GeometryBackend) {
    let prompt = agent_language_reference(backend);
    fs::write(path, format!("{}\n", prompt.trim_end())).expect("write generated agent prompt");
    eprintln!("Wrote {}", path.display());
}

fn write_book_operation_index() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let book_path = root.join("docs/books/ecky-ir/ecky-ir-corpus.md");
    let source = fs::read_to_string(&book_path).expect("read canonical Ecky corpus");
    let mut surface_names = BTreeSet::new();

    for backend in [GeometryBackend::EckyRust, GeometryBackend::Freecad] {
        for entry in supported_surface_reference(backend).entries {
            surface_names.insert(entry.name);
        }
    }

    let table = render_operation_index(&source, &surface_names);
    let surface_table = render_surface_reference();

    let (prefix, after_start) = source
        .split_once(OP_INDEX_START)
        .expect("canonical corpus operation-index start marker");
    let (_, suffix) = after_start
        .split_once(OP_INDEX_END)
        .expect("canonical corpus operation-index end marker");
    let updated = format!("{prefix}{OP_INDEX_START}\n{table}{OP_INDEX_END}{suffix}");
    let (prefix, after_start) = updated
        .split_once(SURFACE_REFERENCE_START)
        .expect("canonical corpus surface-reference start marker");
    let (_, suffix) = after_start
        .split_once(SURFACE_REFERENCE_END)
        .expect("canonical corpus surface-reference end marker");
    let updated = format!(
        "{prefix}{SURFACE_REFERENCE_START}\n{surface_table}{SURFACE_REFERENCE_END}{suffix}"
    );

    fs::write(&book_path, updated).expect("write canonical book operation index");
    eprintln!("Updated {}", book_path.display());
}

fn render_surface_reference() -> String {
    let mut entries = BTreeMap::new();
    for (backend, label) in [
        (GeometryBackend::Freecad, "freecad"),
        (GeometryBackend::EckyRust, "mesh/native"),
    ] {
        for entry in supported_surface_reference(backend).entries {
            let (_, backends): &mut (_, BTreeSet<&str>) = entries
                .entry(entry.name.clone())
                .or_insert_with(|| (entry.clone(), BTreeSet::new()));
            backends.insert(label);
        }
    }

    let mut table = String::from(
        "| Form | Kind | Signature | Backends | Description | Example |\n\
         | --- | --- | --- | --- | --- | --- |\n",
    );
    for (name, (entry, backends)) in entries {
        table.push_str(&format!(
            "| `{}` | {} | `{}` | {} | {} | `{}` |\n",
            markdown_cell(&name),
            markdown_cell(&entry.kind),
            markdown_cell(&entry.signature),
            backends.into_iter().collect::<Vec<_>>().join(", "),
            markdown_cell(&entry.description),
            markdown_cell(&entry.example),
        ));
    }
    table
}

fn markdown_cell(value: &str) -> String {
    value
        .replace('\n', " ")
        .replace('|', "\\|")
        .replace('`', "\\`")
}

fn render_operation_index(source: &str, surface_names: &BTreeSet<String>) -> String {
    let mut section = String::new();
    let mut documented = Vec::new();

    for line in source.lines() {
        if let Some(title) = line.strip_prefix("## ") {
            section = title.trim().to_owned();
            continue;
        }
        let Some(raw_heading) = line.strip_prefix("### ") else {
            continue;
        };
        let Some(name) = raw_heading
            .trim()
            .strip_prefix('`')
            .and_then(|value| value.strip_suffix('`'))
        else {
            continue;
        };
        if surface_names.contains(name) {
            documented.push((name.to_owned(), section.clone()));
        }
    }

    documented.sort_by(|left, right| left.0.cmp(&right.0));
    let mut table = String::from("| Form | Reference |\n| --- | --- |\n");
    for (name, section) in documented {
        table.push_str(&format!(
            "| [`{name}`](#{}) | {section} |\n",
            markdown_anchor(&name)
        ));
    }
    table
}

fn markdown_anchor(value: &str) -> String {
    let mut anchor = String::new();
    let mut separator_pending = false;
    for character in value.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            if separator_pending && !anchor.is_empty() {
                anchor.push('-');
            }
            separator_pending = false;
            anchor.push(character);
        } else {
            separator_pending = true;
        }
    }
    anchor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_index_links_only_documented_surface_forms() {
        let source =
            "## Primitive Signatures\n\n### `box`\n\n### Notes\n\n## Other\n\n### `missing`";
        let surface_names = BTreeSet::from(["box".to_owned(), "undocumented".to_owned()]);

        let table = render_operation_index(source, &surface_names);

        assert!(table.contains("| Form | Reference |"));
        assert!(table.contains("| [`box`](#box) | Primitive Signatures |"));
        assert!(!table.contains("undocumented"));
        assert!(!table.contains("Available backends"));
    }

    #[test]
    fn markdown_anchor_matches_docs_heading_slug() {
        assert_eq!(markdown_anchor("deg->rad"), "deg-rad");
        assert_eq!(markdown_anchor("repeat-union"), "repeat-union");
    }

    #[test]
    fn generated_surface_reference_contains_registry_only_forms() {
        let table = render_surface_reference();

        assert!(table.contains("| `verify` | modelClause |"));
        assert!(table.contains("| `feature` | modelClause |"));
        assert!(table.contains("| `import-step` | cadOp |"));
        assert!(table.contains("mesh/native"));
        assert!(!table.contains("| `assembly` |"));
    }
}
