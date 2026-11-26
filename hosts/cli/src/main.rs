//! `workpiece`: the command line scripts and agents build parts with.
//! What each command does is the library [`workpiece_make`], which the
//! application in a browser runs too; this is the program around it, on
//! the disk.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(error) = workpiece_make::command(&args, &|line| println!("{line}")) {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
