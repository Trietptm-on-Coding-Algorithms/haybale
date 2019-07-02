use inkwell::module::Module;
use inkwell::values::*;
use pitchfork_rs::*;
use std::path::Path;

fn main() {
    env_logger::init();
    // With 0 args, finds zeroes of all the functions in "basic"
    // With 1 arg (a function name), finds zero of that function in "basic"
    // With 2 args (file then function), finds zero of that function in that file
    let firstarg = std::env::args().nth(1);
    let secondarg = std::env::args().nth(2);
    let (modname, funcname): (String, Option<String>) = if let Some(funcname) = secondarg {
        (firstarg.expect("Have second arg, so we must have first arg"), Some(funcname))
    } else if let Some(funcname) = firstarg {
        ("basic".to_owned(), Some(funcname))
    } else {
        ("basic".to_owned(), None)
    };
    let pathstring: String = format!("c_examples/{}/{}.bc", modname, modname);
    let filepath = Path::new(&pathstring);
    let llvm_mod = Module::parse_bitcode_from_path(&filepath).unwrap_or_else(|e| panic!("Failed to parse module at path {}: {}", filepath.display(), e));
    let functions: Box<Iterator<Item = FunctionValue>>;
    if let Some(f) = funcname {
        functions = Box::new(std::iter::once(llvm_mod.get_function(&f).unwrap_or_else(|| panic!("Failed to find function named {}", f))));
    } else {
        functions = Box::new(FunctionIterator::new(&llvm_mod));
    }
    for func in functions {
        println!("Finding zero of function {:?}...", func.get_name());
        if let Some(args) = find_zero_of_func(func) {
            assert_eq!(args.len(), func.count_params() as usize);
            match func.count_params() {
                0 => println!("Function returns zero when passed no arguments\n"),
                1 => println!("Function returns zero when passed the argument {:?}\n", args[0]),
                _ => println!("Function returns zero when passed arguments {:?}\n", args),
            }
        } else {
            println!("Function never returns zero for any values of the arguments\n");
        }
    }
}