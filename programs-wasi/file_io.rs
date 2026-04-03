use std::fs;

fn main() {
    fs::write("/sandbox/test.txt", "howdy from file").unwrap();
    let contents = fs::read_to_string("/sandbox/test.txt").unwrap();
    println!("{contents}");
}
