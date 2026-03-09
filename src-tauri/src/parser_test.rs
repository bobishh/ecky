use rustpython_parser::ast::{self, Stmt};
use rustpython_parser::{parse, Mode};

fn main() {
    let source = "params = {'a': 1, 'b': True, 'c': 'str'}";
    let ast = parse(source, Mode::Module, "<embedded>").unwrap();
    for stmt in ast {
        if let Stmt::Assign(assign) = stmt {
            for target in assign.targets {
                if let ast::Expr::Name(name) = target {
                    if name.id.as_str() == "params" {
                        println!("Found params assignment!");
                    }
                }
            }
        }
    }
}
