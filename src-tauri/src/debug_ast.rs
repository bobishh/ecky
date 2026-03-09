use rustpython_parser::{parse, Mode};
use rustpython_parser::ast::{self, Stmt, Expr};

fn main() {
    let code = "params = {'a': 1}";
    let ast = parse(code, Mode::Module, "<test>").unwrap();
    println!("{:#?}", ast);
}
