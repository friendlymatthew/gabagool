use std::time::{Duration, Instant};

fn main() {
    let before = Instant::now();
    std::thread::sleep(Duration::from_millis(100));
    let elapsed = before.elapsed();

    if elapsed >= Duration::from_millis(100) {
        println!("slept ok");
    } else {
        eprintln!("sleep too short: {:?}", elapsed);
        std::process::exit(1);
    }
}
