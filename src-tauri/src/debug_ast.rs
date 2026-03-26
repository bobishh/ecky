use rustpython_parser::ast::{self, Expr, Stmt};
use rustpython_parser::{parse, Mode};

fn main() {
    let code = "params = {'a': 1}";
    let ast = parse(code, Mode::Module, "<test>").unwrap();
    println!("{:#?}", ast);
}
